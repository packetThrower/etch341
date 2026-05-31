//! `AppView` methods — Diff view: verify diff + file-vs-file compare, region nav, selection copy.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// Drop the last failed verify's chip read-back into the Hex pane
    /// with the differing offsets pre-highlighted, jump to the first,
    /// and select the Hex pane. The diffs become the Find match set, so
    /// the chevrons / Cmd+G step through them. (Typing in Find recomputes
    /// from the search term and takes over — a deliberate switch back to
    /// search.) No-op if the last verify left no stored diff.
    pub fn show_verify_diff(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.verify_diff.as_ref() else {
            return;
        };
        let len = d.file_bytes.len().min(d.chip_bytes.len());
        let (rows, region_rows) = crate::diff::diff_regions(&d.offsets, len);
        let total_diffs = d.offsets.len();
        let file = d.file_bytes.clone();
        let chip = d.chip_bytes.clone();
        // The `d` borrow of `self.verify_diff` ends here; the mutations
        // below need `&mut self`.
        self.push_log(format!(
            "Verify diff: {total_diffs} differing byte(s) across {} region(s) — Cmd/Ctrl+G to step",
            region_rows.len()
        ));
        self.diff_selection = None;
        self.hex_diff = Some(DiffView {
            file,
            chip,
            rows: Arc::new(rows),
            region_rows,
            current: 0,
            total_diffs,
            title: "Verify diff".into(),
            left_label: "file".into(),
            right_label: "chip".into(),
        });
        self.selected = Pane::Hex;
        cx.notify();
    }

    /// Close the diff view, returning the Hex pane to its normal viewer.
    pub fn close_diff(&mut self, cx: &mut Context<Self>) {
        self.hex_diff = None;
        self.diff_selection = None;
        cx.notify();
    }

    /// Pick two files (sequential pickers — A then B) and open their
    /// byte-level diff in the Hex pane. Cancelling either picker aborts.
    /// The Hex pane's "Compare two files…" button drives this.
    pub fn pick_compare_files(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let pick = |title: &'static str, dir: Option<std::path::PathBuf>| {
                let mut dialog = rfd::AsyncFileDialog::new()
                    .set_title(title)
                    .add_filter("Dumps", &["bin", "rom", "eep"])
                    .add_filter("All files", &["*"]);
                if let Some(dir) = dir {
                    dialog = dialog.set_directory(dir);
                }
                dialog.pick_file()
            };

            let Some(handle_a) = pick("Compare — first file (left, red)", start_dir).await else {
                return;
            };
            let path_a = handle_a.path().to_path_buf();
            // Second picker opens in the first file's directory — the
            // two files being compared usually live near each other.
            let dir_b = path_a.parent().map(|p| p.to_path_buf());
            let Some(handle_b) = pick("Compare — second file (right, green)", dir_b).await else {
                return;
            };
            let path_b = handle_b.path().to_path_buf();

            weak.update(cx, |this, cx| this.show_file_diff(path_a, path_b, cx))
                .ok();
        })
        .detach();
    }

    /// Read two files and open their diff in the Hex pane. Surfaces read
    /// errors and the all-match case as an op result rather than a diff.
    pub fn show_file_diff(
        &mut self,
        path_a: std::path::PathBuf,
        path_b: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) {
        let read =
            |p: &std::path::Path| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()));
        let (a, b) = match (read(&path_a), read(&path_b)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                self.set_op_result(false, format!("Compare failed — {e}"));
                cx.notify();
                return;
            }
        };
        let name = |p: &std::path::Path| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string())
        };
        let (name_a, name_b) = (name(&path_a), name(&path_b));
        let offsets = crate::diff::diff_offsets(&a, &b);
        if offsets.is_empty() {
            self.set_op_result(
                true,
                format!("{name_a} and {name_b} are identical ({} bytes)", a.len()),
            );
            cx.notify();
            return;
        }
        let (rows, region_rows) = crate::diff::diff_regions(&offsets, a.len().max(b.len()));
        let total_diffs = offsets.len();
        self.push_log(format!(
            "Compare {name_a} ↔ {name_b}: {total_diffs} differing byte(s) across {} region(s)",
            region_rows.len()
        ));
        self.diff_selection = None;
        self.hex_diff = Some(DiffView {
            file: Arc::new(a),
            chip: Arc::new(b),
            rows: Arc::new(rows),
            region_rows,
            current: 0,
            total_diffs,
            title: "Compare files".into(),
            left_label: name_a,
            right_label: name_b,
        });
        self.selected = Pane::Hex;
        cx.notify();
    }

    /// Scroll the diff view to the next / previous differing region
    /// (wrapping). Bound to the diff view's chevrons + Cmd/Ctrl+G.
    pub fn diff_step_region(&mut self, forward: bool, cx: &mut Context<Self>) {
        let row = match self.hex_diff.as_mut() {
            Some(d) if !d.region_rows.is_empty() => {
                let n = d.region_rows.len();
                d.current = if forward {
                    (d.current + 1) % n
                } else {
                    (d.current + n - 1) % n
                };
                d.region_rows[d.current]
            }
            _ => return,
        };
        self.diff_scroll
            .scroll_to_item_with_offset(row, ScrollStrategy::Top, 0);
        cx.notify();
    }

    /// Anchor a diff-view selection on `side` at `byte`. Shift extends
    /// an existing same-side selection without moving the anchor.
    pub fn diff_begin_select(
        &mut self,
        side: DiffSide,
        byte: usize,
        shift: bool,
        cx: &mut Context<Self>,
    ) {
        self.diff_selection = match (shift, self.diff_selection) {
            (true, Some((s, anchor, _))) if s == side => Some((side, anchor, byte)),
            _ => Some((side, byte, byte)),
        };
        cx.notify();
    }

    /// Extend the diff selection to `byte` while dragging within the
    /// same column. No-op across columns or with no active selection.
    pub fn diff_extend_select(&mut self, side: DiffSide, byte: usize, cx: &mut Context<Self>) {
        if let Some((s, anchor, _)) = self.diff_selection
            && s == side
        {
            self.diff_selection = Some((side, anchor, byte));
            cx.notify();
        }
    }

    /// Copy the diff-view selection (from whichever column it's on) to
    /// the clipboard as space-separated upper-case hex.
    pub fn copy_diff_selection(&mut self, cx: &mut Context<Self>) {
        let Some((side, a, b)) = self.diff_selection else {
            return;
        };
        let Some(diff) = self.hex_diff.as_ref() else {
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let buf = match side {
            DiffSide::File => &diff.file,
            DiffSide::Chip => &diff.chip,
        };
        let end = (hi + 1).min(buf.len());
        if lo >= end {
            return;
        }
        let mut s = String::with_capacity((end - lo) * 3);
        for (i, byte) in buf[lo..end].iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&format!("{byte:02X}"));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(s));
        let label = match side {
            DiffSide::File => "file",
            DiffSide::Chip => "chip",
        };
        self.push_log(format!(
            "Copied {} byte(s) from the {label} side at 0x{lo:08X}",
            end - lo
        ));
    }
}
