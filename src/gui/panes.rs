use super::{AppView, Pane, theme};
use gpui::{
    ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, UniformListScrollHandle, WeakEntity, div,
    prelude::FluentBuilder, px, uniform_list,
};
use gpui_component::input::{Input, InputState};
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
    pub hex_scroll: UniformListScrollHandle,
    pub strings_scroll: UniformListScrollHandle,
    pub hex_highlight_line: Option<usize>,
    pub hex_show_strings: bool,
    pub hex_search_term: &'a str,
    pub hex_search_state: &'a Entity<InputState>,
    pub spi_speed_khz: u32,
    pub prefs_path: Option<&'a Path>,
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
        Pane::Hex => hex_pane(
            inputs.hex_path,
            inputs.hex_bytes,
            inputs.hex_strings,
            inputs.hex_scroll,
            inputs.strings_scroll,
            inputs.hex_highlight_line,
            inputs.hex_show_strings,
            inputs.hex_search_term,
            inputs.hex_search_state,
            cx,
        )
        .into_any_element(),
        Pane::Settings => {
            settings_pane(inputs.spi_speed_khz, inputs.prefs_path, cx).into_any_element()
        }
    }
}

fn read_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Read"))
        .child(body(
            "Auto-detects the chip and dumps its entire contents to a \
             timestamped file in your home directory. Runs on a background \
             thread so the GUI stays responsive — watch the log for progress.",
        ))
        .child(action_button_for(
            "Start read",
            "start-read",
            cx,
            |this, cx| this.start_read(cx),
        ))
}

fn erase_pane(armed: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Erase"))
        .child(body(
            "Erases the entire chip back to 0xFF. DESTRUCTIVE and not \
             undoable — make sure you have a Read backup first. Click \
             the button to arm, then click again to actually erase. \
             Switching panes resets the arm state.",
        ))
        .when(armed, |this| {
            this.child(
                div()
                    .self_start()
                    .px_3()
                    .py_2()
                    .rounded(px(6.0))
                    .bg(theme::warning_amber())
                    .text_color(theme::bench_black())
                    .whitespace_normal()
                    .child("Armed — next click will erase the entire chip."),
            )
        })
        .child(erase_button(armed, cx))
}

fn erase_button(armed: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    // Mirrors `action_button_for` but with a conditional label/bg
    // for the armed state. Stays a sibling helper rather than a new
    // generic so the callsite reads like "erase button at <state>"
    // instead of a long param list.
    let (label, bg) = if armed {
        ("Click again to confirm", theme::caution_red())
    } else {
        ("Erase chip", theme::accent_blue())
    };
    div()
        .id("start-erase")
        .self_start()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(110.0))
        .px_4()
        .py_2()
        .rounded(px(6.0))
        .bg(bg)
        .text_color(theme::text_primary())
        .cursor_pointer()
        .child(label)
        .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
            this.arm_or_fire_erase(cx);
        }))
}

fn write_pane(path: Option<&Path>, armed: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    let (label, bg) = if armed {
        ("Click again to confirm", theme::caution_red())
    } else {
        ("Write chip", theme::accent_blue())
    };
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Write"))
        .child(body(
            "Programs the chip from a file. Erases first, then writes \
             page-by-page, then verifies. DESTRUCTIVE — same arm/confirm \
             protection as Erase. Switching panes resets the arm state.",
        ))
        .child(file_picker_row(
            path,
            "Browse…",
            "pick-write",
            cx,
            |this, cx| this.pick_write_file(cx),
        ))
        .when(armed && path.is_some(), |this| {
            this.child(
                div()
                    .self_start()
                    .px_3()
                    .py_2()
                    .rounded(px(6.0))
                    .bg(theme::warning_amber())
                    .text_color(theme::bench_black())
                    .whitespace_normal()
                    .child("Armed — next click will erase and overwrite the chip."),
            )
        })
        .child(
            div()
                .id("start-write")
                .self_start()
                .flex()
                .items_center()
                .justify_center()
                .min_w(px(110.0))
                .px_4()
                .py_2()
                .rounded(px(6.0))
                .bg(bg)
                .text_color(theme::text_primary())
                .cursor_pointer()
                .child(label)
                .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
                    this.arm_or_fire_write(cx);
                })),
        )
}

fn verify_pane(path: Option<&Path>, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Verify"))
        .child(body(
            "Reads the chip and compares against a file byte-by-byte. \
             Non-destructive; reports the first mismatch's address if any \
             bytes differ.",
        ))
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
        .child(body(
            "SPI clock speed for every op below. Saved immediately to your \
             preferences file; the next op picks up the new value when it \
             opens the CH341A.",
        ))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_tertiary())
                .child("SPI CLOCK SPEED"),
        );

    for &(khz, label) in speeds {
        col = col.child(speed_row(khz, label, current_khz == khz, cx));
    }

    col = col.child(
        div()
            .pt_3()
            .text_size(px(11.0))
            .text_color(theme::text_tertiary())
            .child("PREFERENCES FILE"),
    );
    let path_text = prefs_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unable to determine — $HOME not set)".to_string());
    col = col.child(
        div()
            .text_size(px(12.0))
            .font_family("Menlo")
            .text_color(theme::text_secondary())
            .whitespace_normal()
            .child(path_text),
    );

    col
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

fn hex_pane(
    path: Option<&Path>,
    bytes: Option<Arc<Vec<u8>>>,
    strings: Option<Arc<Vec<(usize, String)>>>,
    hex_scroll: UniformListScrollHandle,
    strings_scroll: UniformListScrollHandle,
    highlight_line: Option<usize>,
    show_strings: bool,
    search_term: &str,
    search_state: &Entity<InputState>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = div()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .py_5()
        .child(heading("Hex viewer"))
        .child(body(
            "Inspect any binary file in hex+ASCII, or extract its \
             printable strings (`strings`-style, ≥4 chars). Browse to \
             pick a file — typically a flash dump from the Read pane. \
             Hex view is clamped to the first 16 KB; strings view scans \
             the whole file.",
        ))
        .child(file_picker_row(
            path,
            "Browse…",
            "pick-hex",
            cx,
            |this, cx| this.pick_hex_file(cx),
        ))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(hex_mode_toggle(show_strings, cx))
                // Search input only appears in Strings mode — it's a
                // no-op in Hex view and the placeholder calling that
                // out felt cluttered. Input has its own background +
                // border styling; an extra wrapper makes the wrapper's
                // edges peek out as an artifact.
                .when(show_strings, |row| {
                    // Input has a white background by default; without
                    // overriding text_color it inherits the dark theme's
                    // foreground (light) → invisible typing on white.
                    // Force dark text on the input only.
                    row.child(
                        div()
                            .flex_1()
                            .text_color(theme::bench_black())
                            .child(Input::new(search_state)),
                    )
                }),
        );

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
            let footer = format!("Showing all {} bytes (0x{:X})", total, total);
            col = col
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme::text_tertiary())
                        .child(footer),
                )
                .child(hex_view(data_arc, hex_scroll, highlight_line));
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
    div()
        .h(px(400.0))
        .border_1()
        .border_color(theme::workshop_glass_strong())
        .rounded(px(6.0))
        .bg(theme::bench_black())
        .px_3()
        .py_2()
        .font_family("Menlo")
        .text_size(px(11.0))
        .text_color(theme::text_secondary())
        .child(
            uniform_list("strings-list", count, move |range, _, _| {
                range
                    .map(|virtual_i| {
                        let real_i = matched_arc[virtual_i];
                        let (offset, s) = &all[real_i];
                        let offset_val = *offset;
                        let weak = weak_view.clone();
                        div()
                            .id(("string-row", virtual_i))
                            .flex()
                            .flex_row()
                            .gap_3()
                            .cursor_pointer()
                            .hover(|d| d.bg(theme::workshop_glass()))
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
                            })
                    })
                    .collect()
            })
            .h_full()
            .track_scroll(&scroll),
        )
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
) -> impl IntoElement {
    let line_count = bytes.len().div_ceil(16);
    div()
        .h(px(400.0))
        .border_1()
        .border_color(theme::workshop_glass_strong())
        .rounded(px(6.0))
        .bg(theme::bench_black())
        .px_3()
        .py_2()
        .font_family("Menlo")
        .text_size(px(11.0))
        .text_color(theme::text_secondary())
        .child(
            uniform_list("hex-lines", line_count, move |range, _, _| {
                range
                    .map(|i| {
                        let start = i * 16;
                        let end = (start + 16).min(bytes.len());
                        // Explicit row height so uniform_list's
                        // measurement of item 0 matches the rendered
                        // height of every subsequent row exactly.
                        // 16px is comfortable at our 11px font.
                        let row = div()
                            .h(px(16.0))
                            .whitespace_nowrap()
                            .child(hex_line(start, &bytes[start..end]));
                        if Some(i) == highlight_line {
                            row.bg(theme::accent_blue_tint())
                                .text_color(theme::text_primary())
                        } else {
                            row
                        }
                    })
                    .collect()
            })
            .h_full()
            .track_scroll(&scroll),
        )
}

fn hex_line(offset: usize, chunk: &[u8]) -> String {
    // Hex bytes — pad short final chunks so the ASCII column lines up.
    let mut hex = String::with_capacity(16 * 3 + 2);
    for (i, b) in chunk.iter().enumerate() {
        if i == 8 {
            hex.push(' '); // extra gap between byte 7 and byte 8
        }
        hex.push_str(&format!("{:02X} ", b));
    }
    for i in chunk.len()..16 {
        if i == 8 {
            hex.push(' ');
        }
        hex.push_str("   ");
    }
    // ASCII representation: printable ASCII byte → glyph, else '.'.
    let ascii: String = chunk
        .iter()
        .map(|&b| {
            if (0x20..0x7F).contains(&b) {
                b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("{:08X}  {}  |{}|", offset, hex.trim_end(), ascii)
}

fn blank_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Blank check"))
        .child(body(
            "Reads the entire chip and confirms every byte is 0xFF. \
             Most useful after an erase — fails with the address of the \
             first non-FF byte if the chip isn't actually blank. A \
             programmed chip (e.g. a VBIOS) will fail at offset 0x0.",
        ))
        .child(action_button_for(
            "Run blank check",
            "start-blank",
            cx,
            |this, cx| this.start_blank_check(cx),
        ))
}

fn detect_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading("Detect"))
        .child(body(
            "Reads the chip's JEDEC ID, looks it up in the bundled chip database, \
             and updates the session header above.",
        ))
        .child(action_button("Refresh", cx))
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
    // `min_w` keeps short buttons (Refresh, Start read) the same size
    // for visual consistency, while longer labels (Run blank check)
    // grow to fit. `flex_none` prevents the button from being stretched
    // by its parent flex column. Horizontal padding pairs with the
    // intrinsic text width.
    div()
        .id(id)
        // `flex_none` only controls main-axis grow/shrink; the parent
        // `flex_col` still stretches us across the cross axis (width).
        // `self_start` opts out so the button hugs its intrinsic
        // width + padding instead of filling the pane.
        .self_start()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(110.0))
        .px_4()
        .py_2()
        .rounded(px(6.0))
        .bg(theme::accent_blue())
        .text_color(theme::text_primary())
        .cursor_pointer()
        .hover(|d| d.bg(theme::accent_blue_hover()))
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}
