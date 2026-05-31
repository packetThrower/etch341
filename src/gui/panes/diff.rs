//! Side-by-side diff rendering. Both the failed-verify diff and the
//! file-vs-file compare feed a `DiffView` through this one view.

// The parent module is this submodule's prelude: `use super::*` pulls
// in its imports (gpui, theme, AppView, DiffView/DiffRow/DiffSide …) and
// the shared widget helpers (styled_button) that the panes build on.
use super::*;

/// Side-by-side verify diff (file vs chip): only the differing runs +
/// context lines, file-hex on the left, chip-hex on the right, with the
/// differing bytes lit in the standard diff colours (red = file, green
/// = chip). Replaces the Hex pane's normal viewer while a diff is open.
pub(super) fn diff_pane(
    diff: &DiffView,
    selection: Option<(DiffSide, usize, usize)>,
    scroll: UniformListScrollHandle,
    font_size: f32,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let region_count = diff.region_rows.len();
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            // `min_w(0)` lets this flex item shrink below its (nowrap)
            // text width instead of shoving the nav/Close buttons off
            // the right edge. The red/left · green/right file mapping is
            // already spelled out in the body paragraph above, so the
            // summary only carries the live count here.
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_color(theme::text_secondary())
                .child(format!(
                    "{} bytes differ across {region_count} region(s).",
                    diff.total_diffs
                )),
        )
        .when(region_count > 1, |row| {
            row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(diff_nav_button("‹ Prev", "diff-prev", cx, |this, cx| {
                        this.diff_step_region(false, cx)
                    }))
                    .child(
                        div()
                            .text_color(theme::text_tertiary())
                            .child(format!("{}/{region_count}", diff.current + 1)),
                    )
                    .child(diff_nav_button("Next ›", "diff-next", cx, |this, cx| {
                        this.diff_step_region(true, cx)
                    })),
            )
        })
        .child(diff_nav_button("Close", "diff-close", cx, |this, cx| {
            this.close_diff(cx)
        }));

    let file = diff.file.clone();
    let chip = diff.chip.clone();
    let rows = diff.rows.clone();
    let weak = cx.entity().downgrade();
    // Mirror the hex viewer's box exactly: a `uniform_list` with
    // `flex_1` fills its column via flex-stretch (a plain scrolling
    // `div` sizes to content instead, which is what kept the box off the
    // window edge). With the box full-width, each row's `w_full`
    // resolves against it and the chip column reaches the right edge.
    let list = uniform_list("diff-list", diff.rows.len(), move |range, _, _| {
        range
            .map(|i| diff_row_view(rows[i], &file, &chip, font_size, selection, weak.clone()))
            .collect::<Vec<_>>()
    })
    .flex_1()
    .min_h(px(0.0))
    .border_1()
    .border_color(theme::workshop_glass_strong())
    .rounded(px(6.0))
    .bg(theme::bench_black())
    .px_3()
    .py_2()
    .track_scroll(&scroll);

    // Same outer shell as the hex pane: flex_1 + w_full + flex_col. The
    // `body()` line (a full-width wrapping paragraph, like every op
    // pane) is what makes the pane fill the viewport width, which gives
    // the list a full width to stretch into.
    div()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .py_5()
        // Dynamic heading + body (the verify case vs two-file compare
        // name the sides differently), inlined because `heading`/`body`
        // take `&'static str`. The wrapping body paragraph is also what
        // makes the pane fill the viewport width (see `body`).
        .child(
            div()
                .text_size(px(18.0))
                .text_color(theme::text_primary())
                .child(diff.title.clone()),
        )
        .child(
            div()
                .text_color(theme::text_secondary())
                .whitespace_normal()
                .child(format!(
                    "Side-by-side: {} on the left (red) and {} on the right (green), \
                     showing only the differing regions plus a couple of context lines. \
                     Drag to select bytes on either side; Cmd/Ctrl+C copies the selection.",
                    diff.left_label, diff.right_label
                )),
        )
        .child(header)
        .child(list)
}

/// A compact button for the diff view's header (Prev / Next / Close).
pub(super) fn diff_nav_button<F>(
    label: &'static str,
    id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    styled_button(id)
        .bg(theme::workshop_glass())
        .text_color(theme::text_secondary())
        .hover(|d| {
            d.bg(theme::workshop_glass_strong())
                .text_color(theme::text_primary())
        })
        .child(label)
        .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| on_click(this, cx)))
}

/// One row of the diff list: a region header or a 16-byte data row
/// (offset · file hex · separator · chip hex), differing bytes lit.
pub(super) fn diff_row_view(
    row: DiffRow,
    file: &[u8],
    chip: &[u8],
    font_size: f32,
    selection: Option<(DiffSide, usize, usize)>,
    weak: WeakEntity<AppView>,
) -> AnyElement {
    match row {
        DiffRow::Header {
            first_diff,
            diff_count,
        } => div()
            .font_family(theme::MONO_FONT)
            .text_size(px(font_size))
            .text_color(theme::text_tertiary())
            .pt_3()
            .pb_1()
            .child(format!(
                "── 0x{first_diff:06X}  ·  {diff_count} byte(s) differ ──"
            ))
            .into_any_element(),
        DiffRow::Bytes { offset } => {
            let mut file_cells = div().flex().flex_row();
            let mut chip_cells = div().flex().flex_row();
            for k in 0..16 {
                let p = offset + k;
                let fb = file.get(p).copied();
                let cb = chip.get(p).copied();
                let differs = fb != cb;
                file_cells = file_cells.child(diff_byte_cell(
                    fb,
                    differs,
                    DiffSide::File,
                    p,
                    in_diff_selection(DiffSide::File, p, selection),
                    weak.clone(),
                ));
                chip_cells = chip_cells.child(diff_byte_cell(
                    cb,
                    differs,
                    DiffSide::Chip,
                    p,
                    in_diff_selection(DiffSide::Chip, p, selection),
                    weak.clone(),
                ));
            }
            // Columns sit adjacent (offset · file · separator · chip),
            // left-aligned inside the full-width box — like the hex
            // viewer packs its content — so a file byte lines up next to
            // its chip counterpart for easy comparison.
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_4()
                .font_family(theme::MONO_FONT)
                .text_size(px(font_size))
                .child(
                    div()
                        .text_color(theme::text_tertiary())
                        .child(format!("{offset:08X}")),
                )
                .child(file_cells)
                .child(div().text_color(theme::text_tertiary()).child("│"))
                .child(chip_cells)
                .into_any_element()
        }
    }
}

/// A single byte cell in the diff. `differs` lights it in the diff
/// colour (red for the file side, green for the chip side); a missing
/// byte (past the shorter buffer) renders blank.
pub(super) fn diff_byte_cell(
    b: Option<u8>,
    differs: bool,
    side: DiffSide,
    offset: usize,
    selected: bool,
    weak: WeakEntity<AppView>,
) -> gpui::Stateful<gpui::Div> {
    let text = b
        .map(|v| format!("{v:02X} "))
        .unwrap_or_else(|| "   ".to_string());
    let fg = if !differs {
        theme::text_secondary()
    } else if side == DiffSide::File {
        theme::diff_removed()
    } else {
        theme::diff_added()
    };
    // Tuple id (distinct per side) keeps each cell interactive; gpui
    // needs an id before .on_mouse_down / .on_mouse_move take effect.
    let id: (&'static str, usize) = match side {
        DiffSide::File => ("diff-file", offset),
        DiffSide::Chip => ("diff-chip", offset),
    };
    let mut d = div().id(id).text_color(fg).child(text);
    if selected {
        // Selection tint wins over the diff colour, like the hex view.
        d = d.bg(theme::selection_tint());
    } else if differs {
        d = d.bg(if side == DiffSide::File {
            theme::diff_removed_bg()
        } else {
            theme::diff_added_bg()
        });
    }
    let weak_down = weak.clone();
    let weak_move = weak;
    d.on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _, app| {
        let shift = ev.modifiers.shift;
        weak_down
            .update(app, |this, cx| {
                this.diff_begin_select(side, offset, shift, cx)
            })
            .ok();
    })
    .on_mouse_move(move |ev: &MouseMoveEvent, _, app| {
        if ev.pressed_button == Some(MouseButton::Left) {
            weak_move
                .update(app, |this, cx| this.diff_extend_select(side, offset, cx))
                .ok();
        }
    })
}

/// True if `offset` on `side` is inside the diff selection.
pub(super) fn in_diff_selection(
    side: DiffSide,
    offset: usize,
    sel: Option<(DiffSide, usize, usize)>,
) -> bool {
    match sel {
        Some((s, a, b)) if s == side => {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            offset >= lo && offset <= hi
        }
        _ => false,
    }
}
