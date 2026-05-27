use super::{AppView, Pane, theme};
use gpui::{
    ClickEvent, Context, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, StatefulInteractiveElement, Styled, UniformListScrollHandle,
    WeakEntity, div, prelude::FluentBuilder, px, uniform_list,
};
use gpui_component::input::{Input, InputState};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

/// Bundle of per-pane state passed from `AppView::render` to keep
/// `render()`'s signature from growing per added pane.
pub struct PaneInputs<'a> {
    pub erase_armed: bool,
    pub write_armed: bool,
    pub write_path: Option<&'a Path>,
    pub verify_path: Option<&'a Path>,
    pub hex_path: Option<&'a Path>,
    pub hex_bytes: Option<Arc<Vec<u8>>>,
    pub hex_strings: Option<Arc<Vec<(usize, String)>>>,
    pub hex_byte_matches: Arc<HashSet<usize>>,
    pub hex_match_total: usize,
    pub hex_current_match: Option<usize>,
    pub hex_scroll: UniformListScrollHandle,
    pub strings_scroll: UniformListScrollHandle,
    pub hex_highlight_line: Option<usize>,
    pub hex_show_strings: bool,
    /// Normalized `(lo, hi)` inclusive byte range. `None` = no
    /// selection. Already normalized by the caller (AppView::selection_range).
    pub hex_selection: Option<(usize, usize)>,
    pub hex_search_term: &'a str,
    pub hex_search_state: &'a Entity<InputState>,
    pub spi_speed_khz: u32,
    /// Current value of the "restore window position on startup"
    /// toggle in Settings. The actual save/restore plumbing lives
    /// in `gui::run` / `on_window_should_close`.
    pub restore_window_bounds: bool,
    pub prefs_path: Option<&'a Path>,
    /// Last-read SR1/SR2/SR3 bytes for the Status pane. `None`
    /// before the user clicks "Read"; populated by
    /// `AppView::start_read_status` once the chip has responded.
    pub status_regs: Option<crate::spi::StatusRegisters>,
}

pub fn render(
    selected: Pane,
    inputs: PaneInputs<'_>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    match selected {
        Pane::Detect => detect_pane(cx).into_any_element(),
        Pane::Read => read_pane(cx).into_any_element(),
        Pane::Erase => erase_pane(inputs.erase_armed, cx).into_any_element(),
        Pane::Write => write_pane(inputs.write_path, inputs.write_armed, cx).into_any_element(),
        Pane::Verify => verify_pane(inputs.verify_path, cx).into_any_element(),
        Pane::Blank => blank_pane(cx).into_any_element(),
        Pane::Status => status_pane(inputs.status_regs, cx).into_any_element(),
        Pane::Hex => hex_pane(
            inputs.hex_path,
            inputs.hex_bytes,
            inputs.hex_strings,
            inputs.hex_byte_matches,
            inputs.hex_match_total,
            inputs.hex_current_match,
            inputs.hex_scroll,
            inputs.strings_scroll,
            inputs.hex_highlight_line,
            inputs.hex_show_strings,
            inputs.hex_selection,
            inputs.hex_search_term,
            inputs.hex_search_state,
            cx,
        )
        .into_any_element(),
        Pane::Settings => {
            settings_pane(
                inputs.spi_speed_khz,
                inputs.restore_window_bounds,
                inputs.prefs_path,
                cx,
            )
            .into_any_element()
        }
    }
}

fn read_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Read",
        "Auto-detects the chip and dumps its entire contents to a \
         timestamped file in your home directory. Runs on a background \
         thread so the GUI stays responsive — watch the log for progress.",
    )
    .child(action_button_for(
        "Start read",
        "start-read",
        cx,
        |this, cx| this.start_read(cx),
    ))
}

fn erase_pane(armed: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Erase",
        "Erases the entire chip back to 0xFF. DESTRUCTIVE and not \
         undoable — make sure you have a Read backup first. Click \
         the button to arm, then click again to actually erase. \
         Switching panes resets the arm state.",
    )
    .when(armed, |this| {
        this.child(armed_warning(
            "Armed — next click will erase the entire chip.",
        ))
    })
    .child(armable_button(
        "Erase chip",
        "Click again to confirm",
        "start-erase",
        armed,
        cx,
        |this, cx| this.arm_or_fire_erase(cx),
    ))
}

fn write_pane(path: Option<&Path>, armed: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Write",
        "Programs the chip from a file. Erases first, then writes \
         page-by-page, then verifies. DESTRUCTIVE — same arm/confirm \
         protection as Erase. Switching panes resets the arm state.",
    )
    .child(file_picker_row(
        path,
        "Browse…",
        "pick-write",
        cx,
        |this, cx| this.pick_write_file(cx),
    ))
    .when(armed && path.is_some(), |this| {
        this.child(armed_warning(
            "Armed — next click will erase and overwrite the chip.",
        ))
    })
    .child(armable_button(
        "Write chip",
        "Click again to confirm",
        "start-write",
        armed,
        cx,
        |this, cx| this.arm_or_fire_write(cx),
    ))
}

fn verify_pane(path: Option<&Path>, cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Verify",
        "Reads the chip and compares against a file byte-by-byte. \
         Non-destructive; reports the first mismatch's address if any \
         bytes differ.",
    )
    .child(file_picker_row(
        path,
        "Browse…",
        "pick-verify",
        cx,
        |this, cx| this.pick_verify_file(cx),
    ))
    .child(action_button_for(
        "Verify",
        "start-verify",
        cx,
        |this, cx| this.start_verify(cx),
    ))
}

/// Path display + Browse button row. Path text wraps if long.
fn file_picker_row<F>(
    path: Option<&Path>,
    button_label: &'static str,
    button_id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let display = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(no file selected)".to_string());
    let path_color = if path.is_some() {
        theme::text_primary()
    } else {
        theme::text_tertiary()
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .whitespace_normal()
                .text_color(path_color)
                .text_size(px(12.0))
                .font_family("Menlo")
                .child(display),
        )
        .child(action_button_for(button_label, button_id, cx, on_click))
}

fn settings_pane(
    current_khz: u32,
    restore_window_bounds: bool,
    prefs_path: Option<&Path>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    // Matches `ch341::SUPPORTED_SPEEDS_KHZ` — the set the CH341A
    // accepts via the standard I²C-stream set-speed command.
    let speeds: &[(u32, &str)] = &[
        (20, "20 kHz — slowest, most signal-tolerant"),
        (100, "100 kHz — conservative default"),
        (400, "400 kHz"),
        (750, "750 kHz — fastest reliable rate (recommended)"),
    ];

    let mut col = div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Settings"))
        .child(section_label("SPI CLOCK SPEED"))
        .child(body(
            "Clock rate for every SPI op (Detect / Read / Erase / Write / \
             Verify / Blank-check). Saved immediately; the next op picks \
             up the new value when it opens the CH341A.",
        ));

    for &(khz, label) in speeds {
        col = col.child(speed_row(khz, label, current_khz == khz, cx));
    }

    col = col
        .child(section_label("WINDOW"))
        .child(body(
            "When enabled, the window's last position and size are saved \
             on close and restored the next time you launch etch341. Off by \
             default — the window opens centered at 1200 × 800.",
        ))
        .child(toggle_row(
            "Restore window position on startup",
            "toggle-restore-bounds",
            restore_window_bounds,
            cx,
            |this, cx| this.toggle_restore_window_bounds(cx),
        ));

    col = col.child(section_label("PREFERENCES FILE"));
    let path_text = prefs_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unable to determine — $HOME not set)".to_string());
    // Path text + "Open folder" button on the same row, matching
    // the `file_picker_row` shape so the settings pane reads as a
    // consistent family. Button hidden when there's no real path —
    // nothing useful to open if $HOME wasn't set.
    col = col.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(12.0))
                    .font_family("Menlo")
                    .text_color(theme::text_secondary())
                    .whitespace_normal()
                    .child(path_text),
            )
            .when_some(prefs_path, |row, _| {
                row.child(action_button_for(
                    "Open folder",
                    "open-prefs-folder",
                    cx,
                    |this, cx| this.open_prefs_folder(cx),
                ))
            }),
    );

    col
}

/// Small-caps section header inside the settings pane. Matches the
/// existing visual treatment used for "SPI CLOCK SPEED" /
/// "PREFERENCES FILE"; factored out to keep the new "WINDOW"
/// header consistent with the rest by construction.
fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .pt_3()
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(text)
}

/// Switch-style row: label on the left, a small pill on the right
/// that fills when `active`. Mirrors `speed_row`'s shape (clickable
/// the whole way across, hover tint, accent-blue when on) so the
/// settings pane looks consistent. Used for boolean prefs.
fn toggle_row<F>(
    label: &'static str,
    id: &'static str,
    active: bool,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let knob_x = if active { px(18.0) } else { px(2.0) };
    let track_bg = if active {
        theme::accent_blue()
    } else {
        theme::workshop_glass_strong()
    };
    div()
        .id(id)
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(|d| d.bg(theme::workshop_glass()))
        .child(
            div()
                .flex_1()
                .text_color(theme::text_primary())
                .child(label),
        )
        .child(
            // Track + thumb. Fixed pixel sizes so the switch reads
            // crisp at any window scale; absolute-positioned thumb
            // slides with `left` between the on/off positions.
            div()
                .relative()
                .w(px(36.0))
                .h(px(20.0))
                .rounded(px(10.0))
                .bg(track_bg)
                .child(
                    div()
                        .absolute()
                        .top(px(2.0))
                        .left(knob_x)
                        .w(px(16.0))
                        .h(px(16.0))
                        .rounded(px(8.0))
                        .bg(theme::text_primary()),
                ),
        )
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}

fn speed_row(
    khz: u32,
    label: &'static str,
    active: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let dot_outer_color = if active {
        theme::accent_blue()
    } else {
        theme::workshop_glass_strong()
    };
    let mut row = div()
        .id(("speed", khz as u64))
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .child(
            div()
                .w(px(12.0))
                .h(px(12.0))
                .rounded_full()
                .border_1()
                .border_color(dot_outer_color)
                .flex()
                .items_center()
                .justify_center()
                .when(active, |d| {
                    d.child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(theme::accent_blue()),
                    )
                }),
        )
        .child(
            div()
                .text_color(if active {
                    theme::text_primary()
                } else {
                    theme::text_secondary()
                })
                .child(label.to_string()),
        )
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                this.set_spi_speed(khz, cx);
            }),
        );
    if active {
        row = row.bg(theme::accent_blue_tint());
    }
    row
}

// The hex pane sits at the join point of the file picker, the
// search/filter input, the matches/selection overlays, the strings
// list, and the hex view — each one needing its own slice of state
// to render. Bundling into a single struct is the natural refactor
// (PaneInputs already does this for the dispatch layer above) but
// the rendering callsite is exactly one place, so a per-field
// allow is cheaper than another wrapper layer for now.
#[allow(clippy::too_many_arguments)]
fn hex_pane(
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
    selection: Option<(usize, usize)>,
    search_term: &str,
    search_state: &Entity<InputState>,
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
        .child(heading("Hex viewer"))
        .child(body(
            "Inspect any binary file in hex+ASCII, or extract its \
             printable strings (≥4 chars). The Find bar below works in \
             both modes — typing highlights matching bytes in Hex and \
             filters the Strings list. Press Enter on `0xOFFSET` to jump \
             to that address; on a pattern, Enter jumps to the first \
             match. Chevrons step between matches.",
        ))
        .child(file_picker_row(
            path,
            "Browse…",
            "pick-hex",
            cx,
            |this, cx| this.pick_hex_file(cx),
        ))
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
                    div()
                        .flex_1()
                        .text_color(theme::bench_black())
                        .child(Input::new(search_state)),
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
                        .text_size(px(11.0))
                        .text_color(theme::text_tertiary())
                        .child(footer),
                )
                .child(hex_view(
                    data_arc,
                    hex_scroll,
                    highlight_line,
                    byte_matches,
                    selection,
                    cx,
                ));
        }
    } else {
        col = col.child(
            div()
                .text_color(theme::text_tertiary())
                .text_size(px(12.0))
                .child("(no file loaded — click Browse to pick one)"),
        );
    }
    col
}

/// Counter + prev/next chevrons for the find navigator. Shows `i+1/N`
/// when a cursor is set, just `N` when matches exist but the user
/// hasn't navigated yet. Chevrons wrap around at the ends.
fn find_nav(current: Option<usize>, total: usize, cx: &mut Context<AppView>) -> impl IntoElement {
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

fn find_arrow<F>(
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
fn hex_mode_toggle(show_strings: bool, cx: &mut Context<AppView>) -> impl IntoElement {
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

fn toggle_button<F>(
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
        btn = btn.bg(theme::accent_blue_tint());
    } else {
        btn = btn.hover(|d| d.bg(theme::workshop_glass_strong()));
    }
    btn
}

/// Render the filtered strings list via `uniform_list` — only the
/// visible rows are formatted, so even 100k+ string lists scroll
/// smoothly. Each row is clickable: click → switch to Hex view and
/// scroll to that byte (handled via `AppView::jump_to_hex_offset`).
fn strings_view(
    all: Arc<Vec<(usize, String)>>,
    matched: Vec<usize>,
    needle: String,
    scroll: UniformListScrollHandle,
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
                            .text_color(theme::accent_blue())
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
    .font_family("Menlo")
    .text_size(px(11.0))
    .text_color(theme::text_secondary())
    .track_scroll(&scroll)
}

/// Render one strings-view row with the matched substring (if any)
/// painted in the accent colour. Single-line (whitespace_nowrap) so
/// `uniform_list` can rely on a fixed row height; long strings clip
/// at the viewport edge.
fn highlight_string_row(haystack: &str, needle: &str) -> impl IntoElement {
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
                theme::accent_blue()
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
fn hex_view(
    bytes: Arc<Vec<u8>>,
    scroll: UniformListScrollHandle,
    highlight_line: Option<usize>,
    byte_matches: Arc<HashSet<usize>>,
    selection: Option<(usize, usize)>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let line_count = bytes.len().div_ceil(16);
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
                    weak.clone(),
                );
                if Some(i) == highlight_line {
                    row.bg(theme::accent_blue_tint())
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
    .font_family("Menlo")
    .text_size(px(11.0))
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
fn hex_row(
    offset: usize,
    chunk: &[u8],
    matches: &HashSet<usize>,
    selection: Option<(usize, usize)>,
    weak: WeakEntity<AppView>,
) -> gpui::Div {
    let mut row = div()
        .h(px(16.0))
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
            row = row.child(div().child(" "));
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

    // ASCII column: separator + 16 chars + separator. Same match +
    // selection highlighting as the hex column; mouse events on the
    // ASCII cells extend the selection too, so the user can drag
    // across either side of the row.
    row = row.child(
        div()
            .text_color(theme::text_tertiary())
            .child(" |".to_string()),
    );
    for i in 0..16 {
        let cell = if let Some(&b) = chunk.get(i) {
            let pos = offset + i;
            let is_printable = (0x20..0x7F).contains(&b);
            let glyph = if is_printable {
                (b as char).to_string()
            } else {
                ".".to_string()
            };
            let base_color = if is_printable {
                theme::text_primary()
            } else {
                theme::text_tertiary()
            };
            row = row.child(byte_cell(
                pos,
                glyph,
                base_color,
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
            .child("|".to_string()),
    );
    row
}

/// One interactive byte cell — used by both the hex and ASCII
/// columns. Encapsulates the color/tint precedence (selection over
/// match over base) and the mouse-down / mouse-move handlers that
/// drive selection.
fn byte_cell(
    pos: usize,
    glyph: String,
    base_color: gpui::Hsla,
    is_match: bool,
    is_selected: bool,
    weak: WeakEntity<AppView>,
) -> gpui::Stateful<gpui::Div> {
    let color = if is_match {
        theme::accent_blue()
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
        d = d.bg(theme::accent_blue_tint());
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
fn in_selection(pos: usize, selection: Option<(usize, usize)>) -> bool {
    match selection {
        Some((lo, hi)) => pos >= lo && pos <= hi,
        None => false,
    }
}

/// Tier color for a hex byte. Tuning the brightness here is how the
/// "scan the dump for structure" feeling emerges — null and 0xFF
/// runs should sink, printable runs should float.
fn hex_color_for(b: u8) -> gpui::Hsla {
    match b {
        0x00 | 0xFF => theme::text_tertiary(),
        0x20..=0x7E => theme::text_primary(),
        _ => theme::text_secondary(),
    }
}

fn status_pane(
    regs: Option<crate::spi::StatusRegisters>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Status registers",
        "Reads SR1 / SR2 / SR3 and decodes the standard bits. Useful \
         for diagnosing writes that silently fail (block-protect set) \
         or quad-mode issues (QE clear).",
    );
    col = col.child(action_button_for(
        "Read status",
        "read-status",
        cx,
        |this, cx| this.start_read_status(cx),
    ));
    if let Some(r) = regs {
        col = col.child(status_register_block(
            "SR1",
            r.sr1,
            true,
            &[
                ("WIP", r.wip().to_string()),
                ("WEL", r.wel().to_string()),
                ("BP", r.bp().to_string()),
                ("TB", r.tb().to_string()),
                ("SEC/BP3", r.sec_or_bp3().to_string()),
                ("SRP0", r.srp0().to_string()),
            ],
        ));
        col = col.child(status_register_block(
            "SR2",
            r.sr2,
            r.sr2_present(),
            &[
                ("SRP1", r.srp1().to_string()),
                ("QE", r.qe().to_string()),
                ("LB", r.lb().to_string()),
                ("CMP", r.cmp().to_string()),
                ("SUS", r.sus().to_string()),
            ],
        ));
        col = col.child(status_register_block(
            "SR3",
            r.sr3,
            r.sr3_present(),
            &[
                ("ADP", r.adp().to_string()),
                ("WPS", r.wps().to_string()),
                ("DRV", r.drv().to_string()),
                ("HOLD/RST", r.hold_rst().to_string()),
            ],
        ));
        // Same gotcha-surfacing note the CLI prints — most common
        // reason a user is on this pane is "writes silently failing"
        // due to BP bits, so call it out directly.
        if r.bp() != 0 || r.sec_or_bp3() {
            col = col.child(armed_warning(
                "SR1 has block-protect bits set — writes/erases to the protected range \
                 will silently fail. Clear BP[2:0] (and SEC/BP3 if set) via WRSR before \
                 programming.",
            ));
        }
    }
    col
}

/// One register row inside `status_pane` — section label, raw hex
/// + binary, and a wrapped grid of decoded bit name → value pairs.
/// `present == false` means the chip returned `0xFF` (no response)
/// and we hide the decoded bits since they'd just be noise.
fn status_register_block(
    name: &'static str,
    value: u8,
    present: bool,
    bits: &[(&'static str, String)],
) -> gpui::Div {
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_tertiary())
                .child(name),
        )
        .child(
            div()
                .text_size(px(12.0))
                .font_family("Menlo")
                .text_color(if present {
                    theme::text_primary()
                } else {
                    theme::text_tertiary()
                })
                .child(format!("0x{value:02X}  0b{value:08b}")),
        );
    if !present {
        return div()
            .flex()
            .flex_col()
            .gap_1()
            .child(header)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme::text_tertiary())
                    .whitespace_normal()
                    .child(format!("(chip didn't respond — likely doesn't implement {name})")),
            );
    }
    // Bit-grid: each entry is "NAME: value" in Menlo at 12px, laid
    // out as flex-wrap so narrow windows wrap rather than overflow.
    let mut grid = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1()
        .text_size(px(12.0))
        .font_family("Menlo")
        .text_color(theme::text_secondary());
    for (label, v) in bits {
        grid = grid.child(
            div()
                .child(format!("{label}={v}")),
        );
    }
    div().flex().flex_col().gap_1().child(header).child(grid)
}

fn blank_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Blank check",
        "Reads the entire chip and confirms every byte is 0xFF. \
         Most useful after an erase — fails with the address of the \
         first non-FF byte if the chip isn't actually blank. A \
         programmed chip (e.g. a VBIOS) will fail at offset 0x0.",
    )
    .child(action_button_for(
        "Run blank check",
        "start-blank",
        cx,
        |this, cx| this.start_blank_check(cx),
    ))
}

fn detect_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Detect",
        "Reads the chip's JEDEC ID, looks it up in the bundled chip database, \
         and updates the session header above.",
    )
    .child(action_button("Detect chip", cx))
}

/// Shared outer shell for the operation panes (Detect / Read / Erase
/// / Write / Verify / Blank). Returns a `flex_col` div with the
/// standard pane padding + gap, the heading, and the body paragraph
/// already added — callers chain on the per-pane extras (file
/// picker, armed warning, action button) via `.child(...)`. Keeps
/// "what a pane looks like" in one place so future tweaks (e.g.
/// changing the body color or the gap between rows) land here
/// instead of in six near-identical copies.
fn op_pane(heading_text: &'static str, body_text: &'static str) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading(heading_text))
        .child(body(body_text))
}

fn heading(text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(18.0))
        .text_color(theme::text_primary())
        .child(text)
}

fn body(text: &'static str) -> impl IntoElement {
    // `whitespace_normal` overrides gpui's default `nowrap`; without
    // this, long descriptions push the pane wider than its viewport
    // and the right edge clips off-screen.
    div()
        .text_color(theme::text_secondary())
        .whitespace_normal()
        .child(text)
}

/// The amber "Armed — next click will <thing>" pill shown by the
/// two-stage destructive panes (Erase, Write) between the body and
/// the action button when the user has primed the operation.
fn armed_warning(text: &'static str) -> impl IntoElement {
    div()
        .self_start()
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .bg(theme::warning_amber())
        .text_color(theme::bench_black())
        .text_size(px(13.0))
        .whitespace_normal()
        .child(text)
}

fn action_button(label: &'static str, cx: &mut Context<AppView>) -> impl IntoElement {
    // Legacy single-purpose button used by the Detect pane.
    action_button_for(label, label, cx, |this, cx| this.refresh_detect(cx))
}

fn action_button_for<F>(
    label: &'static str,
    id: &'static str,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    styled_button(id)
        .bg(theme::accent_blue())
        .hover(|d| d.bg(theme::accent_blue_hover()))
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}

/// Two-stage destructive button used by Erase and Write. First click
/// arms (the parent sets `armed = true` and re-renders); second
/// click fires. Background flips amber → red as a "you're about to
/// do something irreversible" cue. The handler closure runs on
/// every click — the AppView decides whether this is an arm or a
/// fire based on its own `*_armed` flag.
fn armable_button<F>(
    idle_label: &'static str,
    armed_label: &'static str,
    id: &'static str,
    armed: bool,
    cx: &mut Context<AppView>,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    let (label, bg) = if armed {
        (armed_label, theme::caution_red())
    } else {
        (idle_label, theme::accent_blue())
    };
    styled_button(id)
        .bg(bg)
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}

/// Shared button skeleton (sizing, rounding, text color, layout).
/// Callers add `.bg(...)`, `.hover(...)`, `.child(label)`, and
/// `.on_click(...)`. Tweaks to "what an op-pane button looks like"
/// (padding, font size, min width) belong here.
fn styled_button(id: &'static str) -> gpui::Stateful<gpui::Div> {
    // `min_w` keeps short buttons (Refresh, Start read) the same size
    // for visual consistency, while longer labels (Run blank check)
    // grow to fit. `self_start` opts the button out of the parent
    // `flex_col` cross-axis stretch so it hugs its intrinsic width +
    // padding rather than filling the pane. Tightened from the
    // original `px_4`/`py_2` + default text size — the bulky CTAs
    // read as out of proportion against the rest of the chrome.
    div()
        .id(id)
        .self_start()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(96.0))
        .px_3()
        .py_1p5()
        .rounded(px(6.0))
        .text_size(px(13.0))
        .text_color(theme::text_primary())
        .cursor_pointer()
}
