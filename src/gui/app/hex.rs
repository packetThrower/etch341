//! `AppView` methods — Hex viewer: font zoom, find/jump, byte selection + copy, file picker.

// `impl AppView` blocks may live in any module of the crate; this
// submodule adds one. `use crate::gui::*` pulls the parent module's
// prelude (gpui, AppView, shared types + the `push_log`/`set_op_result`
// infra these methods call) into scope.
use crate::gui::*;

impl AppView {
    /// Bump the hex view font size by `delta` (typically ±1), clamped
    /// to the supported range and saved to prefs. No-op at the
    /// clamp boundary so we don't churn the file on repeated
    /// keypresses past the limit.
    ///
    /// Re-scrolls to the same row index before returning so the user
    /// stays parked at the same byte offset across zoom steps —
    /// without this the underlying pixel scroll offset maps to a
    /// different row at the new row height and the viewport "jumps."
    /// The row-height arithmetic mirrors `hex_view`'s `+5` formula.
    pub fn nudge_hex_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.prefs.hex_font_size + delta)
            .clamp(crate::prefs::HEX_FONT_MIN, crate::prefs::HEX_FONT_MAX);
        if (next - self.prefs.hex_font_size).abs() < f32::EPSILON {
            return;
        }
        let top = uniform_list_top_index(&self.hex_scroll, self.prefs.hex_font_size + 5.0);
        self.prefs.hex_font_size = next;
        self.hex_scroll.scroll_to_item(top, ScrollStrategy::Top);
        self.persist_font_size(cx);
    }

    /// Same for the strings view. Separate from hex on purpose —
    /// the strings list and hex grid have different density needs.
    /// Strings rows have no explicit height — uniform_list measures
    /// them from natural text content, which is `font_size *
    /// line_height` plus a hair. `font_size + 4` matches what gpui
    /// produces with default line-height for mono fonts in the
    /// 8-24px range.
    pub fn nudge_strings_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.prefs.strings_font_size + delta)
            .clamp(crate::prefs::HEX_FONT_MIN, crate::prefs::HEX_FONT_MAX);
        if (next - self.prefs.strings_font_size).abs() < f32::EPSILON {
            return;
        }
        let top = uniform_list_top_index(&self.strings_scroll, self.prefs.strings_font_size + 4.0);
        self.prefs.strings_font_size = next;
        self.strings_scroll.scroll_to_item(top, ScrollStrategy::Top);
        self.persist_font_size(cx);
    }

    /// Keybinding handler: cmd/ctrl + `=` / `+`. Only fires when
    /// the Hex pane is selected; routes to whichever sub-view is
    /// currently visible (hex grid or strings list).
    pub fn hex_zoom_in(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            self.nudge_strings_font(1.0, cx);
        } else {
            self.nudge_hex_font(1.0, cx);
        }
    }

    /// Keybinding handler: cmd/ctrl + `-`.
    pub fn hex_zoom_out(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            self.nudge_strings_font(-1.0, cx);
        } else {
            self.nudge_hex_font(-1.0, cx);
        }
    }

    /// Keybinding handler: cmd/ctrl + `0`. Resets the active sub-
    /// view's font to the original default. Same scroll-preserve
    /// trick as the nudge handlers.
    pub fn hex_zoom_reset(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            let top =
                uniform_list_top_index(&self.strings_scroll, self.prefs.strings_font_size + 4.0);
            self.prefs.strings_font_size = crate::prefs::HEX_FONT_DEFAULT;
            self.strings_scroll.scroll_to_item(top, ScrollStrategy::Top);
        } else {
            let top = uniform_list_top_index(&self.hex_scroll, self.prefs.hex_font_size + 5.0);
            self.prefs.hex_font_size = crate::prefs::HEX_FONT_DEFAULT;
            self.hex_scroll.scroll_to_item(top, ScrollStrategy::Top);
        }
        self.persist_font_size(cx);
    }

    /// Settings → Hex viewer → "Reset to defaults" button. Resets
    /// *both* font sizes — Cmd+0 only resets the active sub-view,
    /// but a settings-level reset is the natural "put it all back"
    /// affordance. No scroll-preserve since the user is on the
    /// Settings pane, not Hex.
    pub fn reset_hex_fonts(&mut self, cx: &mut Context<Self>) {
        self.prefs.hex_font_size = crate::prefs::HEX_FONT_DEFAULT;
        self.prefs.strings_font_size = crate::prefs::HEX_FONT_DEFAULT;
        self.persist_font_size(cx);
    }

    /// Shared save-and-redraw path for font-size changes. Failures
    /// only surface in the activity log (silent on success — these
    /// happen on every keystroke during a zoom session).
    fn persist_font_size(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("font size save failed: {e}"));
        }
        cx.notify();
    }

    /// Refresh `hex_byte_matches` + `hex_first_match` from the current
    /// `hex_bytes` and `hex_search_term`. Called from the search
    /// subscription (wired in `AppView::new`) and after loading a new
    /// file — hence `pub(crate)` rather than private.
    pub(crate) fn recompute_hex_matches(&mut self) {
        self.hex_current_match = None;
        let Some(bytes) = self.hex_bytes.as_ref() else {
            self.hex_byte_matches = Arc::new(HashSet::new());
            self.hex_match_starts = Vec::new();
            return;
        };
        let pattern = parse_hex_needle(&self.hex_search_term);
        if pattern.is_empty() || bytes.len() < pattern.len() {
            self.hex_byte_matches = Arc::new(HashSet::new());
            self.hex_match_starts = Vec::new();
            return;
        }
        let pat_len = pattern.len();
        let mut set = HashSet::new();
        let mut starts = Vec::new();
        for i in 0..=bytes.len() - pat_len {
            let hit = (0..pat_len).all(|j| byte_match_ci(bytes[i + j], pattern[j]));
            if hit {
                starts.push(i);
                for j in 0..pat_len {
                    set.insert(i + j);
                }
            }
        }
        self.hex_byte_matches = Arc::new(set);
        self.hex_match_starts = starts;
    }

    /// PressEnter on the find input. `0x...` → explicit offset jump;
    /// anything else steps to the next match (wraps from `None` cursor
    /// to first match).
    pub fn find_enter(&mut self, cx: &mut Context<Self>) {
        let raw = self.hex_search_state.read(cx).value().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
            self.jump_via_input(trimmed, cx);
            return;
        }
        self.find_next(cx);
    }

    /// Advance the find cursor to the next match (wrap at the end).
    /// Fresh search → lands on match 0.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        let total = self.hex_match_starts.len();
        if total == 0 {
            self.push_log("Find: no matches".into());
            cx.notify();
            return;
        }
        let next_idx = match self.hex_current_match {
            Some(i) => (i + 1) % total,
            None => 0,
        };
        self.hex_current_match = Some(next_idx);
        let offset = self.hex_match_starts[next_idx];
        self.push_log(format!(
            "Find: match {}/{} at 0x{:X}",
            next_idx + 1,
            total,
            offset
        ));
        self.jump_to_hex_offset(offset, cx);
    }

    /// Step the find cursor back by one match (wrap at the start).
    /// Fresh search → lands on the last match.
    pub fn find_prev(&mut self, cx: &mut Context<Self>) {
        let total = self.hex_match_starts.len();
        if total == 0 {
            self.push_log("Find: no matches".into());
            cx.notify();
            return;
        }
        let prev_idx = match self.hex_current_match {
            Some(i) => (i + total - 1) % total,
            None => total - 1,
        };
        self.hex_current_match = Some(prev_idx);
        let offset = self.hex_match_starts[prev_idx];
        self.push_log(format!(
            "Find: match {}/{} at 0x{:X}",
            prev_idx + 1,
            total,
            offset
        ));
        self.jump_to_hex_offset(offset, cx);
    }

    /// Swap the Hex pane between raw-bytes view and extracted-strings view.
    pub fn set_hex_strings_mode(&mut self, show_strings: bool, cx: &mut Context<Self>) {
        self.hex_show_strings = show_strings;
        cx.notify();
    }

    /// Wired to clicks on string-list rows: switch to Hex view and
    /// scroll the hex `uniform_list` so the byte at `offset` is
    /// centered in the viewport. Scroll position takes effect on the
    /// next render (after `cx.notify()` triggers it).
    /// Parse the `Jump:` input value and dispatch to
    /// `jump_to_hex_offset`. Accepts `0xFA00`, `0XFA00`, or bare hex
    /// `FA00`. Logs an explanatory error on bad input or out-of-range
    /// offsets — leaves the input intact so the user can correct it.
    pub fn jump_via_input(&mut self, raw: &str, cx: &mut Context<Self>) {
        // Strip ALL whitespace so `0x FA 00` and `55 AA` both parse.
        // The address is logically the concatenation of the hex digits
        // typed; users sometimes type a byte sequence with spaces.
        let condensed: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        if condensed.is_empty() {
            return;
        }
        let stripped = condensed
            .strip_prefix("0x")
            .or_else(|| condensed.strip_prefix("0X"))
            .unwrap_or(&condensed);
        let parsed = usize::from_str_radix(stripped, 16);
        match parsed {
            Ok(offset) => {
                let size = self.hex_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
                if size == 0 {
                    self.push_log("Jump: no file loaded".into());
                    cx.notify();
                    return;
                }
                if offset >= size {
                    self.push_log(format!(
                        "Jump: 0x{:X} is past end of file (size 0x{:X})",
                        offset, size
                    ));
                    cx.notify();
                    return;
                }
                self.push_log(format!("Jump to 0x{:X}", offset));
                self.jump_to_hex_offset(offset, cx);
            }
            Err(_) => {
                self.push_log(format!(
                    "Jump: \u{201C}{}\u{201D} isn't a valid hex offset",
                    raw
                ));
                cx.notify();
            }
        }
    }

    pub fn jump_to_hex_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.hex_show_strings = false;
        let line = offset / 16;
        // Land the highlighted line a few rows below the top of the
        // viewport so the user sees a bit of context above it. Using
        // `Top` with an offset is more predictable than `Center`,
        // which depends on measured item height and was drifting by
        // a few lines.
        self.hex_scroll
            .scroll_to_item_with_offset(line, ScrollStrategy::Top, 3);
        self.hex_highlight_line = Some(line);
        cx.notify();
    }

    /// Selection as a normalized `(lo, hi)` inclusive range. Returns
    /// `None` if nothing is selected or `hex_bytes` is empty.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let (a, b) = self.hex_selection?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// Mouse-down on a hex byte cell. Anchors a new selection — or,
    /// if shift is held and a selection already exists, extends it
    /// without moving the anchor. Sets `hex_selecting` so subsequent
    /// drag-moves can extend.
    pub fn begin_select(&mut self, byte: usize, shift: bool, cx: &mut Context<Self>) {
        if shift {
            if let Some((anchor, _)) = self.hex_selection {
                self.hex_selection = Some((anchor, byte));
            } else {
                self.hex_selection = Some((byte, byte));
            }
        } else {
            self.hex_selection = Some((byte, byte));
        }
        self.hex_selecting = true;
        cx.notify();
    }

    /// Mouse-move while the left button is held inside the hex view.
    /// No-op outside an active drag.
    pub fn extend_select(&mut self, byte: usize, cx: &mut Context<Self>) {
        if !self.hex_selecting {
            return;
        }
        if let Some((anchor, _)) = self.hex_selection {
            self.hex_selection = Some((anchor, byte));
            cx.notify();
        }
    }

    /// Mouse-up — drag is over, but the selection persists for Cmd+C.
    pub fn end_select(&mut self, _cx: &mut Context<Self>) {
        self.hex_selecting = false;
    }

    /// Copy the current hex-view selection to the system clipboard as
    /// space-separated upper-case hex (e.g. "DE AD BE EF"). Gated on
    /// the Hex pane being visible — Cmd+C in any other context (Write
    /// pane, focused Input, etc.) is a no-op here so the OS / Input
    /// widget's own copy still works.
    pub fn copy_hex_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        let Some((lo, hi)) = self.selection_range() else {
            return;
        };
        let Some(bytes) = self.hex_bytes.as_ref() else {
            return;
        };
        let end = (hi + 1).min(bytes.len());
        if lo >= end {
            return;
        }
        let slice = &bytes[lo..end];
        let mut s = String::with_capacity(slice.len() * 3);
        for (i, b) in slice.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&format!("{:02X}", b));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(s));
        self.push_log(format!(
            "Copied {} byte{} from 0x{:08X}",
            end - lo,
            if end - lo == 1 { "" } else { "s" },
            lo
        ));
    }

    /// Open the file picker, load the chosen file into memory, and
    /// stash for the Hex pane to render. Files up to a few MB are fine
    /// to hold in memory; the renderer caps the visible window separately.
    ///
    /// **Deferred via cx.spawn:** NSOpenPanel (and its Linux/Windows
    /// equivalents) pump their own modal event loop. If we open the
    /// dialog synchronously from inside a click handler, AppKit
    /// dispatches pending events from the focused Input back into a
    /// gpui context that's still holding our `&mut AppView` borrow,
    /// and gpui panics with "RefCell already borrowed". Running on a
    /// foreground async task lets the click-handler borrow drop first.
    pub fn pick_hex_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_hex_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            let read_result = std::fs::read(&path);
            weak.update(cx, |this, cx| {
                match read_result {
                    Ok(bytes) => {
                        let bytes_arc = Arc::new(bytes);
                        let strings = extract_strings(&bytes_arc, 4);
                        this.push_log(format!(
                            "Loaded hex view: {} ({} bytes, {} strings)",
                            path.display(),
                            bytes_arc.len(),
                            strings.len()
                        ));
                        if let Some(parent) = path.parent() {
                            this.prefs.last_hex_dir = Some(parent.to_path_buf());
                            let _ = this.prefs.save();
                        }
                        this.hex_input_path = Some(path);
                        this.hex_bytes = Some(bytes_arc);
                        this.hex_strings = Some(Arc::new(strings));
                        this.recompute_hex_matches();
                    }
                    Err(e) => this.push_log(format!("Hex view load failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
