//! Hex viewer + strings pane: file inspection, find, byte selection.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;

// The hex pane sits at the join point of the file picker, the
// search/filter input, the matches/selection overlays, the strings
// list, and the hex view — each one needing its own slice of state
// to render. Bundling into a single struct is the natural refactor
// (PaneInputs already does this for the dispatch layer above) but
// the rendering callsite is exactly one place, so a per-field
// allow is cheaper than another wrapper layer for now.
#[allow(clippy::too_many_arguments)]
pub(super) fn hex_pane(
    path: Option<&Path>,
    bytes: Option<Arc<Vec<u8>>>,
    strings: Option<Arc<Vec<(usize, String)>>>,
    byte_matches: Arc<HashSet<usize>>,
    match_total: usize,
    current_match: Option<usize>,
    hex_scroll: UniformListScrollHandle,
    strings_scroll: UniformListScrollHandle,
    highlight_line: Option<usize>,
    show_strings: bool,
    show_legend: bool,
    selection: Option<(usize, usize)>,
    search_term: &str,
    search_state: &Entity<InputState>,
    hex_font_size: f32,
    strings_font_size: f32,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = div()
        // Absorb the pane wrapper's height via `flex_1` so the inner
        // hex_view's own `flex_1` has something definite to claim.
        // `min_h(0)` keeps flex from refusing to shrink when the
        // window is small.
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .py_5()
        // Title row: heading on the left, "Compare two files…" pushed to
        // the top-right as a secondary (glass) header action.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(heading("Hex viewer"))
                .child(
                    styled_button("hex-compare")
                        .bg(theme::workshop_glass())
                        .text_color(theme::text_secondary())
                        .hover(|d| {
                            d.bg(theme::workshop_glass_strong())
                                .text_color(theme::text_primary())
                        })
                        .child("Compare two files…")
                        .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
                            this.pick_compare_files(cx)
                        })),
                ),
        )
        .child(body(
            "Inspect any binary file in hex+ASCII, or extract its \
             printable strings (≥4 chars). The Find bar below works in \
             both modes — typing highlights matching bytes in Hex and \
             filters the Strings list. Press Enter on `0xOFFSET` to jump \
             to that address; on a pattern, Enter jumps to the first \
             match. Chevrons step between matches.",
        ))
        // File picker in a titled GroupBox, matching the other panes'
        // file-selection controls (Verify / Write / I²C).
        .child(
            GroupBox::new()
                .id("hex-file-box")
                .outline()
                .max_w(px(680.0))
                .title("File to inspect")
                .child(bordered_file_row(path, "pick-hex", cx, |this, cx| {
                    this.pick_hex_file(cx)
                })),
        )
        // Unified Find input — outside the tab so it applies to both
        // Hex and Strings views. Force dark text because the Input's
        // default white background otherwise renders the typed text
        // invisible against the dark theme's inherited foreground.
        // Counter + prev/next buttons appear to the right when there
        // are matches.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(
                    // Wrapper used to set `text_color(bench_black())`
                    // back when gpui-component's Input rendered with a
                    // white background — dark text was needed for
                    // contrast. After `Theme::change(ThemeMode::Dark)`
                    // landed in `gui::run`, the Input picks a dark
                    // background and a light text colour from the theme;
                    // the forced bench-black wrapper here re-darkened
                    // the inherited text colour, producing the
                    // dark-on-dark "typed characters are invisible"
                    // bug. Drop the override — let the Input use the
                    // theme's `text` slot.
                    div().flex_1().child(Input::new(search_state)),
                )
                .when(match_total > 0, |row| {
                    row.child(find_nav(current_match, match_total, cx))
                }),
        )
        .child(hex_mode_toggle(show_strings, cx));

    if let Some(data_arc) = bytes {
        let data: &[u8] = data_arc.as_slice();
        if show_strings {
            let all = strings.unwrap_or_else(|| Arc::new(Vec::new()));
            let needle = search_term.to_ascii_lowercase();
            // Filter by index, not by clone — passes the indices into
            // uniform_list's closure which dereferences into `all`. No
            // per-row String clones; per-keystroke cost stays O(N)
            // search + O(matches) usize push.
            let matched: Vec<usize> = if needle.is_empty() {
                (0..all.len()).collect()
            } else {
                all.iter()
                    .enumerate()
                    .filter(|(_, (_, s))| s.to_ascii_lowercase().contains(&needle))
                    .map(|(i, _)| i)
                    .collect()
            };
            let footer = if needle.is_empty() {
                format!(
                    "Found {} printable run(s) of \u{2265} 4 chars in 0x{:X} bytes",
                    all.len(),
                    data.len()
                )
            } else {
                format!(
                    "{} of {} run(s) match \u{201C}{}\u{201D}",
                    matched.len(),
                    all.len(),
                    search_term
                )
            };
            col = col
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme::text_tertiary())
                        .child(footer),
                )
                .child(strings_view(
                    all,
                    matched,
                    search_term.to_string(),
                    strings_scroll,
                    strings_font_size,
                    cx.entity().downgrade(),
                ));
        } else {
            let total = data.len();
            // Selection summary takes the footer slot when present —
            // it's the more actionable signal (Cmd+C copies it). Falls
            // back to the file-size readout otherwise.
            let footer = match selection {
                Some((lo, hi)) => format!(
                    "Selection: 0x{:08X}..0x{:08X} ({} byte{})",
                    lo,
                    hi,
                    hi - lo + 1,
                    if hi == lo { "" } else { "s" }
                ),
                None => format!("Showing all {} bytes (0x{:X})", total, total),
            };
            col = col
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.0))
                        .text_color(theme::text_tertiary())
                        .child(div().child(footer))
                        .child(legend_toggle(show_legend, cx)),
                )
                .when(show_legend, |c| c.child(hex_legend()))
                .child(hex_view(
                    data_arc,
                    hex_scroll,
                    highlight_line,
                    byte_matches,
                    selection,
                    hex_font_size,
                    cx,
                ));
        }
    } else {
        col = col.child(
            div()
                .text_color(theme::text_tertiary())
                .text_size(px(12.0))
                .child("(no file loaded, click Browse to pick one)"),
        );
    }
    col
}

/// Counter + prev/next chevrons for the find navigator. Shows `i+1/N`
/// when a cursor is set, just `N` when matches exist but the user
/// hasn't navigated yet. Chevrons wrap around at the ends.
pub(super) fn find_nav(
    current: Option<usize>,
    total: usize,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let counter_text = match current {
        Some(i) => format!("{}/{}", i + 1, total),
        None => format!("{} match{}", total, if total == 1 { "" } else { "es" }),
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(find_arrow("\u{2039}", "find-prev", cx, |this, cx| {
            this.find_prev(cx)
        }))
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_secondary())
                .min_w(px(70.0))
                .child(counter_text),
        )
        .child(find_arrow("\u{203A}", "find-next", cx, |this, cx| {
            this.find_next(cx)
        }))
}

pub(super) fn find_arrow<F>(
    label: &'static str,
    id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    div()
        .id(id)
        .w(px(24.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .cursor_pointer()
        .text_color(theme::text_secondary())
        .text_size(px(14.0))
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}

/// Two-button segmented toggle: Hex on the left, Strings on the right.
/// The active option carries the accent tint; the other is muted.
pub(super) fn hex_mode_toggle(show_strings: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    let hex_active = !show_strings;
    div()
        .self_start()
        .flex()
        .flex_row()
        .gap_1()
        .child(toggle_button(
            "Hex",
            "hex-mode-hex",
            hex_active,
            cx,
            |t, c| t.set_hex_strings_mode(false, c),
        ))
        .child(toggle_button(
            "Strings",
            "hex-mode-strings",
            show_strings,
            cx,
            |t, c| t.set_hex_strings_mode(true, c),
        ))
}

pub(super) fn toggle_button<F>(
    label: &'static str,
    id: &'static str,
    active: bool,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let mut btn = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded(px(6.0))
        .cursor_pointer()
        .text_size(px(12.0))
        .text_color(if active {
            theme::text_primary()
        } else {
            theme::text_secondary()
        })
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        );
    if active {
        btn = btn.bg(theme::accent_tint());
    } else {
        btn = btn.hover(|d| d.bg(theme::workshop_glass_strong()));
    }
    btn
}

/// Render the filtered strings list via `uniform_list` — only the
/// visible rows are formatted, so even 100k+ string lists scroll
/// smoothly. Each row is clickable: click → switch to Hex view and
/// scroll to that byte (handled via `AppView::jump_to_hex_offset`).
pub(super) fn strings_view(
    all: Arc<Vec<(usize, String)>>,
    matched: Vec<usize>,
    needle: String,
    scroll: UniformListScrollHandle,
    font_size: f32,
    weak_view: WeakEntity<AppView>,
) -> impl IntoElement {
    let matched_arc = Arc::new(matched);
    let count = matched_arc.len();
    // Same shape as `hex_view`: style chained directly on the
    // uniform_list so its flex_1 sees the hex_pane parent.
    uniform_list("strings-list", count, move |range, _, _| {
        range
            .map(|virtual_i| {
                let real_i = matched_arc[virtual_i];
                let (offset, s) = &all[real_i];
                let offset_val = *offset;
                let weak = weak_view.clone();
                let mut row = div()
                    .id(("string-row", virtual_i))
                    .flex()
                    .flex_row()
                    .gap_3()
                    .cursor_pointer()
                    .hover(|d| d.bg(theme::workshop_glass_strong()))
                    .child(
                        div()
                            .text_color(theme::accent())
                            .child(format!("{:08X}", offset_val)),
                    )
                    .child(highlight_string_row(s, &needle))
                    .on_click(move |_: &ClickEvent, _, app| {
                        weak.update(app, |this, cx| {
                            this.jump_to_hex_offset(offset_val, cx);
                        })
                        .ok();
                    });
                // Ledger-paper stripe — every other row gets a
                // subtle bg to track horizontally without losing
                // your spot.
                if virtual_i % 2 == 1 {
                    row = row.bg(theme::workshop_glass());
                }
                row
            })
            .collect()
    })
    .flex_1()
    .min_h(px(0.0))
    .border_1()
    .border_color(theme::workshop_glass_strong())
    .rounded(px(6.0))
    .bg(theme::bench_black())
    .px_3()
    .py_2()
    .font_family(theme::MONO_FONT)
    .text_size(px(font_size))
    .text_color(theme::text_secondary())
    .track_scroll(&scroll)
}

/// Render one strings-view row with the matched substring (if any)
/// painted in the accent colour. Single-line (whitespace_nowrap) so
/// `uniform_list` can rely on a fixed row height; long strings clip
/// at the viewport edge.
pub(super) fn highlight_string_row(haystack: &str, needle: &str) -> impl IntoElement {
    let row = div().flex().flex_row().flex_1().whitespace_nowrap();
    if needle.is_empty() {
        return row.child(
            div()
                .text_color(theme::text_primary())
                .child(haystack.to_string()),
        );
    }
    let lower_h = haystack.to_ascii_lowercase();
    let lower_n = needle.to_ascii_lowercase();
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut cursor = 0;
    while let Some(pos) = lower_h[cursor..].find(&lower_n) {
        let start = cursor + pos;
        let end = start + needle.len();
        if start > cursor {
            segments.push((haystack[cursor..start].to_string(), false));
        }
        segments.push((haystack[start..end].to_string(), true));
        cursor = end;
    }
    if cursor < haystack.len() {
        segments.push((haystack[cursor..].to_string(), false));
    }
    row.children(segments.into_iter().map(|(text, hit)| {
        div()
            .text_color(if hit {
                theme::accent()
            } else {
                theme::text_primary()
            })
            .child(text)
    }))
}

// `extract_strings` lives in `super` (gui::mod) so AppView's
// file-load path can cache the result; the panes module no longer
// computes it per render.

/// Render bytes as 16-byte-per-line hex + ASCII via `uniform_list`,
/// so only the visible rows are formatted. Renders the full file
/// regardless of size (32 MB chip → 2 M lines, scrolls fine because
/// only ~30 rows are alive at a time).
pub(super) fn hex_view(
    bytes: Arc<Vec<u8>>,
    scroll: UniformListScrollHandle,
    highlight_line: Option<usize>,
    byte_matches: Arc<HashSet<usize>>,
    selection: Option<(usize, usize)>,
    font_size: f32,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let line_count = bytes.len().div_ceil(16);
    // Row height tracks font size so glyphs don't clip when the user
    // zooms in. +5 matches the historical 16px row at the 11px
    // default and scales linearly from there.
    let row_height = font_size + 5.0;
    // WeakEntity captured into the per-row closures so each byte
    // cell's mouse handler can update AppView via `weak.update(...)`.
    // Strong handles would extend AppView's lifetime past its window.
    let weak = cx.entity().downgrade();
    let weak_up = weak.clone();
    // Style applied directly to `uniform_list`. Earlier shape had the
    // styling on a wrapper div and `flex_1` on the inner uniform_list
    // — but uniform_list's internal layout doesn't honour `flex_1`
    // through a non-flex wrapper, so it collapsed to the smallest
    // row. Hoisting the style to the uniform_list itself, with
    // `flex_1 + min_h(0)` so its hex_pane parent's flex_col allocates
    // it the remaining vertical space, makes it actually fill.
    uniform_list("hex-lines", line_count, move |range, _, _| {
        range
            .map(|i| {
                let start = i * 16;
                let end = (start + 16).min(bytes.len());
                let row = hex_row(
                    start,
                    &bytes[start..end],
                    &byte_matches,
                    selection,
                    row_height,
                    weak.clone(),
                );
                if Some(i) == highlight_line {
                    row.bg(theme::accent_tint())
                } else if (i / 8) % 2 == 1 {
                    row.bg(theme::workshop_glass())
                } else {
                    row
                }
            })
            .collect()
    })
    .flex_1()
    .min_h(px(0.0))
    .border_1()
    .border_color(theme::workshop_glass_strong())
    .rounded(px(6.0))
    .bg(theme::bench_black())
    .px_3()
    .py_2()
    .font_family(theme::MONO_FONT)
    .text_size(px(font_size))
    .text_color(theme::text_secondary())
    .track_scroll(&scroll)
    // Drag ends on mouse-up anywhere inside the hex view. A release
    // outside this region leaves `hex_selecting=true`, but the next
    // mouse-down clears it anyway.
    .on_mouse_up(MouseButton::Left, move |_, _, app| {
        weak_up.update(app, |this, cx| this.end_select(cx)).ok();
    })
}

/// Render one hex line as a flex_row of small text spans, each with
/// its own color based on the byte's class. Three tiers:
///   - bright (printable ASCII 0x20-0x7E): "content" bytes pop
///   - dim   (null + 0xFF): padding / erased regions fade into the bg
///   - mid   (control + high bytes): structured data, less prominent
///     than text but more than padding
///
/// ASCII column on the right uses the same brightness mapping.
pub(super) fn hex_row(
    offset: usize,
    chunk: &[u8],
    matches: &HashSet<usize>,
    selection: Option<(usize, usize)>,
    row_height: f32,
    weak: WeakEntity<AppView>,
) -> gpui::Div {
    let mut row = div()
        .h(px(row_height))
        .flex()
        .flex_row()
        .whitespace_nowrap()
        .child(
            div()
                .text_color(theme::text_tertiary())
                .child(format!("{:08X}  ", offset)),
        );

    // Hex columns: 16 byte slots, extra gap after the 8th. Matched
    // bytes (search hits) get the accent-blue color + tint background;
    // selected bytes get a neutral selection tint (overlays match tint
    // when both apply — selection wins because it's the user's active
    // intent).
    for i in 0..16 {
        if i == 8 {
            // Dotted soft divider splitting the row into two 8-byte
            // groups — makes a byte's offset easier to count to by eye.
            row = row.child(
                div()
                    .text_color(theme::text_tertiary())
                    .child("┊ ".to_string()),
            );
        }
        let cell = if let Some(&b) = chunk.get(i) {
            let pos = offset + i;
            row = row.child(byte_cell(
                pos,
                format!("{:02X} ", b),
                hex_color_for(b),
                matches.contains(&pos),
                in_selection(pos, selection),
                weak.clone(),
            ));
            continue;
        } else {
            div().child("   ")
        };
        row = row.child(cell);
    }

    // ASCII column: solid panel break, then 8 + dotted split + 8 chars,
    // then panel close. Same match + selection highlighting as the hex
    // column; mouse events on the ASCII cells extend the selection too,
    // so the user can drag across either side of the row.
    row = row.child(
        div()
            .text_color(theme::text_tertiary())
            .child("│ ".to_string()),
    );
    for i in 0..16 {
        if i == 8 {
            row = row.child(
                div()
                    .text_color(theme::text_tertiary())
                    .child("┊".to_string()),
            );
        }
        let cell = if let Some(&b) = chunk.get(i) {
            let pos = offset + i;
            let is_printable = (0x20..0x7F).contains(&b);
            let glyph = if is_printable {
                (b as char).to_string()
            } else {
                ".".to_string()
            };
            // Same byte-category colour as the hex cell so a run of one
            // category reads as a band across both panels.
            row = row.child(byte_cell(
                pos,
                glyph,
                hex_color_for(b),
                matches.contains(&pos),
                in_selection(pos, selection),
                weak.clone(),
            ));
            continue;
        } else {
            div().child(" ".to_string())
        };
        row = row.child(cell);
    }
    row = row.child(
        div()
            .text_color(theme::text_tertiary())
            .child(" │".to_string()),
    );
    row
}

/// One interactive byte cell — used by both the hex and ASCII
/// columns. Encapsulates the color/tint precedence (selection over
/// match over base) and the mouse-down / mouse-move handlers that
/// drive selection.
pub(super) fn byte_cell(
    pos: usize,
    glyph: String,
    base_color: gpui::Hsla,
    is_match: bool,
    is_selected: bool,
    weak: WeakEntity<AppView>,
) -> gpui::Stateful<gpui::Div> {
    let color = if is_match {
        theme::accent()
    } else {
        base_color
    };
    let mut d = div()
        // Tuple id keeps each cell uniquely interactive — gpui requires
        // an id before .on_mouse_down / .on_mouse_move take effect.
        .id(("hex-cell", pos))
        .text_color(color)
        .child(glyph);
    if is_selected {
        d = d.bg(theme::selection_tint());
    } else if is_match {
        d = d.bg(theme::accent_tint());
    }
    let weak_down = weak.clone();
    let weak_move = weak;
    d.on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, _, app| {
        let shift = ev.modifiers.shift;
        weak_down
            .update(app, |this, cx| this.begin_select(pos, shift, cx))
            .ok();
    })
    .on_mouse_move(move |ev: &MouseMoveEvent, _, app| {
        if ev.pressed_button == Some(MouseButton::Left) {
            weak_move
                .update(app, |this, cx| this.extend_select(pos, cx))
                .ok();
        }
    })
}

/// True if `pos` is inside the (already-normalized) selection range.
pub(super) fn in_selection(pos: usize, selection: Option<(usize, usize)>) -> bool {
    match selection {
        Some((lo, hi)) => pos >= lo && pos <= hi,
        None => false,
    }
}

/// Per-byte colour by byte category — how the "scan the dump for
/// structure" feeling emerges. Cyan = printable graphic, green
/// = whitespace + other control ASCII, gold = non-ASCII; the blank bytes
/// (`0x00` null, `0xFF` erased) sink so real data floats. Drives both
/// the hex and the ASCII columns so the two panels read the same.
pub(super) fn hex_color_for(b: u8) -> gpui::Hsla {
    match b {
        0x00 | 0xFF => theme::hex_null(),
        0x21..=0x7E => theme::hex_printable(),
        0x80..=0xFE => theme::hex_nonascii(),
        // 0x01..=0x20 + 0x7F: ASCII whitespace + other control.
        _ => theme::hex_ascii_other(),
    }
}

/// The "?" toggle next to the Hex footer that expands the byte-colour
/// legend. Subtly highlighted while open.
fn legend_toggle(open: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    let (bg, fg) = if open {
        (theme::workshop_glass_strong(), theme::text_primary())
    } else {
        (theme::workshop_glass(), theme::text_secondary())
    };
    div()
        .id("hex-legend-toggle")
        .flex()
        .items_center()
        .justify_center()
        .size(px(16.0))
        .rounded(px(4.0))
        .bg(bg)
        .text_color(fg)
        .cursor_pointer()
        .hover(|d| {
            d.bg(theme::workshop_glass_strong())
                .text_color(theme::text_primary())
        })
        .child("?")
        .on_click(
            cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| this.toggle_hex_legend(cx)),
        )
}

/// Byte-colour legend shown under the footer when the "?" is expanded:
/// a sample byte in each category colour next to what the colour means.
fn hex_legend() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_4()
        .pt_1()
        .text_size(px(11.0))
        .child(legend_item("41", "printable", theme::hex_printable()))
        .child(legend_item(
            "0A",
            "whitespace / control",
            theme::hex_ascii_other(),
        ))
        .child(legend_item("C3", "non-ASCII", theme::hex_nonascii()))
        .child(legend_item(
            "FF",
            "null / erased (00 / FF)",
            theme::hex_null(),
        ))
}

fn legend_item(sample: &'static str, label: &'static str, color: gpui::Hsla) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(
            div()
                .font_family(theme::MONO_FONT)
                .text_color(color)
                .child(sample),
        )
        .child(div().text_color(theme::text_tertiary()).child(label))
}
