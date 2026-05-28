use super::{AppView, Pane, theme};
use gpui::{
    ClickEvent, Context, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, StatefulInteractiveElement, Styled, UniformListScrollHandle,
    WeakEntity, div, prelude::FluentBuilder, px, uniform_list,
};
use gpui_component::group_box::{GroupBox, GroupBoxVariants as _};
use gpui_component::input::{Input, InputState};
use gpui_component::radio::{Radio, RadioGroup};
use gpui_component::{Sizable as _, Size};
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
    /// Security (OTP) registers from the most recent OTP-pane read.
    /// `None` before the user clicks "Read security registers".
    pub otp_regs: Option<&'a [crate::ops::OtpRegister]>,
    /// Target register (1/2/3) for the OTP pane's erase / write
    /// controls, the selected file, and the two arm flags.
    pub otp_target_register: u8,
    pub otp_write_path: Option<&'a Path>,
    pub otp_erase_armed: bool,
    pub otp_write_armed: bool,
    /// Save directory for Read pane dumps. `None` means "$HOME"
    /// (the original fallback). Settings exposes a Browse button
    /// to set it; the Read pane body surfaces the current value
    /// so the user knows where the next dump will land.
    pub read_output_dir: Option<&'a Path>,
    /// Most recent Detect result for the Detect pane's chip-info
    /// card. `None` until the user clicks Detect chip.
    pub detect_result: Option<&'a crate::gui::DetectInfo>,
    /// Parsed SFDP table from the most recent Detect run, shown
    /// as a second card inside the Detect pane when present.
    /// Decoupled from `detect_result` because some chips have a
    /// valid SFDP magic but a BFPT we can't decode (or vice
    /// versa).
    pub detect_sfdp: Option<&'a crate::sfdp::Sfdp>,
    /// Font size (px) for the hex+ASCII view inside the Hex pane.
    /// Persisted in `prefs.toml` and adjustable on the fly with
    /// Cmd/Ctrl + / - / 0.
    pub hex_font_size: f32,
    /// Same idea for the strings list inside the Hex pane.
    pub strings_font_size: f32,
    /// Render activity-log timestamps in the system's local time
    /// zone (true) or UTC (false, default). Surfaces in Settings →
    /// Log timestamps; storage stays raw UTC seconds either way.
    pub timestamp_local: bool,
}

pub fn render(
    selected: Pane,
    inputs: PaneInputs<'_>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    match selected {
        Pane::Detect => {
            detect_pane(inputs.detect_result, inputs.detect_sfdp, cx).into_any_element()
        }
        Pane::Read => read_pane(cx).into_any_element(),
        Pane::Erase => erase_pane(inputs.erase_armed, cx).into_any_element(),
        Pane::Write => write_pane(inputs.write_path, inputs.write_armed, cx).into_any_element(),
        Pane::Verify => verify_pane(inputs.verify_path, cx).into_any_element(),
        Pane::Blank => blank_pane(cx).into_any_element(),
        Pane::Status => status_pane(inputs.status_regs, cx).into_any_element(),
        Pane::Otp => otp_pane(
            inputs.otp_regs,
            inputs.otp_target_register,
            inputs.otp_write_path,
            inputs.otp_erase_armed,
            inputs.otp_write_armed,
            cx,
        )
        .into_any_element(),
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
            inputs.hex_font_size,
            inputs.strings_font_size,
            cx,
        )
        .into_any_element(),
        Pane::Settings => settings_pane(
            inputs.spi_speed_khz,
            inputs.restore_window_bounds,
            inputs.read_output_dir,
            inputs.prefs_path,
            inputs.hex_font_size,
            inputs.strings_font_size,
            inputs.timestamp_local,
            cx,
        )
        .into_any_element(),
    }
}

fn read_pane(cx: &mut Context<AppView>) -> impl IntoElement {
    op_pane(
        "Read",
        "Dumps the chip to a timestamped file. Pick the save directory \
         in Settings → Read save location.",
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
         undoable. Make sure you have a Read backup first. Click \
         the button to arm, then click again to actually erase. \
         Switching panes resets the arm state.",
    )
    .when(armed, |this| {
        this.child(armed_warning(
            "Armed: next click will erase the entire chip.",
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
         page-by-page, then verifies. DESTRUCTIVE: same arm/confirm \
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
            "Armed: next click will erase and overwrite the chip.",
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
                .font_family(theme::MONO_FONT)
                .child(display),
        )
        .child(action_button_for(button_label, button_id, cx, on_click))
}

#[allow(clippy::too_many_arguments)]
fn settings_pane(
    current_khz: u32,
    restore_window_bounds: bool,
    read_output_dir: Option<&Path>,
    prefs_path: Option<&Path>,
    hex_font_size: f32,
    strings_font_size: f32,
    timestamp_local: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    // Matches `ch341::SUPPORTED_SPEEDS_KHZ` — the set the CH341A
    // accepts via the standard I²C-stream set-speed command.
    let speeds: &[(u32, &str)] = &[
        (20, "20 kHz, slowest, most signal-tolerant"),
        (100, "100 kHz, conservative default"),
        (400, "400 kHz"),
        (750, "750 kHz, fastest reliable rate (recommended)"),
    ];

    // SPI clock speed section: RadioGroup + descriptive body.
    let selected = speeds.iter().position(|&(k, _)| k == current_khz);
    let speed_values: Vec<u32> = speeds.iter().map(|&(k, _)| k).collect();
    let mut radio_group = RadioGroup::vertical("spi-speed")
        .selected_index(selected)
        .on_click(cx.listener(move |this, ix: &usize, _, cx| {
            if let Some(&khz) = speed_values.get(*ix) {
                this.set_spi_speed(khz, cx);
            }
        }));
    for &(_, label) in speeds {
        radio_group = radio_group.child(Radio::new(label).label(label).with_size(Size::Small));
    }
    let speed_box = GroupBox::new()
        .id("settings-speed")
        .outline()
        .title("SPI clock speed")
        .child(body(
            "Clock rate for every SPI op (Detect / Read / Erase / Write / \
             Verify / Blank-check). Saved immediately; the next op picks \
             up the new value when it opens the CH341A.",
        ))
        .child(radio_group);

    // Read save location section.
    let read_dir_text = read_output_dir
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| {
            std::env::var("HOME")
                .map(|h| format!("{h} (default)"))
                .unwrap_or_else(|_| "(default: $HOME, not set)".to_string())
        });
    let read_box = GroupBox::new()
        .id("settings-read")
        .outline()
        .title("Read save location")
        .child(body(
            "Where the Read pane saves chip dumps. Defaults to your home \
             directory; pick a folder to override.",
        ))
        .child(
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
                        .font_family(theme::MONO_FONT)
                        .text_color(theme::text_secondary())
                        .whitespace_normal()
                        .child(read_dir_text),
                )
                .child(action_button_for(
                    "Browse…",
                    "pick-read-output",
                    cx,
                    |this, cx| this.pick_read_output_dir(cx),
                )),
        );

    // Window section.
    let window_box = GroupBox::new()
        .id("settings-window")
        .outline()
        .title("Window")
        .child(body(
            "When enabled, the window's last position and size are saved \
             on close and restored the next time you launch etch341. Off by \
             default; the window opens centered at 1200 × 800.",
        ))
        .child(toggle_row(
            "Restore window position on startup",
            "toggle-restore-bounds",
            restore_window_bounds,
            cx,
            |this, cx| this.toggle_restore_window_bounds(cx),
        ));

    // Hex viewer section: two font-size rows (hex grid + strings
    // list). Mirrors the Cmd/Ctrl + / - / 0 keybindings — both
    // surfaces mutate the same prefs values, so changes round-trip
    // either way.
    let hex_box = GroupBox::new()
        .id("settings-hex")
        .outline()
        .title("Hex viewer")
        .child(body(
            "Font size for the hex+ASCII view and the strings list \
             inside the Hex pane. Adjustable on the fly with Cmd \
             +/- and Cmd 0 on macOS, or Ctrl on Windows / Linux — \
             the active sub-view changes when you toggle between \
             Hex and Strings.",
        ))
        .child(font_size_row(
            "Hex view",
            hex_font_size,
            "hex-font-minus",
            "hex-font-plus",
            cx,
            |this, cx| this.nudge_hex_font(-1.0, cx),
            |this, cx| this.nudge_hex_font(1.0, cx),
        ))
        .child(font_size_row(
            "Strings view",
            strings_font_size,
            "strings-font-minus",
            "strings-font-plus",
            cx,
            |this, cx| this.nudge_strings_font(-1.0, cx),
            |this, cx| this.nudge_strings_font(1.0, cx),
        ))
        .child(
            // Right-aligned reset chip. Sized to match the +/- chips
            // (24px tall, padded for the longer label) so the
            // GroupBox doesn't get a chunky CTA at the bottom.
            div().flex().flex_row().justify_end().child(
                div()
                    .id("hex-font-reset")
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(24.0))
                    .px_3()
                    .rounded(px(4.0))
                    .text_size(px(12.0))
                    .text_color(theme::text_primary())
                    .bg(theme::workshop_glass_strong())
                    .hover(|d| d.bg(theme::workshop_glass()))
                    .cursor_pointer()
                    .child("Reset to defaults")
                    .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
                        this.reset_hex_fonts(cx);
                    })),
            ),
        );

    // Log timestamps section: UTC (default) vs system local time.
    // Storage is always raw UTC seconds; this toggle only changes
    // how the activity log renders — both past and new lines
    // re-format on flip.
    let log_box = GroupBox::new()
        .id("settings-log")
        .outline()
        .title("Log timestamps")
        .child(body(
            "Activity log entries always store UTC under the hood — \
             this only changes how they're displayed. Enable to render \
             both existing and new lines in the system's local time \
             zone; disable to show UTC like an old-school server log.",
        ))
        .child(toggle_row(
            "Show local time in the activity log",
            "toggle-timestamp-local",
            timestamp_local,
            cx,
            |this, cx| this.set_timestamp_local(!this.prefs.timestamp_local, cx),
        ));

    // Preferences file section. Button hidden when there's no real
    // path — nothing useful to open if $HOME wasn't set.
    let path_text = prefs_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unable to determine; $HOME not set)".to_string());
    let prefs_box = GroupBox::new()
        .id("settings-prefs")
        .outline()
        .title("Preferences file")
        .child(
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
                        .font_family(theme::MONO_FONT)
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

    // Two-level structure: a flex-row centering wrapper, then a
    // flex-col content column with `flex_1 + max_w(680)`. flex_1
    // makes the column grow on the main (horizontal) axis up to
    // its max-width, and `justify_center` on the wrapper centers
    // the column when there's slack. This gives the column a
    // *definite* width on wide windows, which is what the body
    // text needs to actually wrap at the right point instead of
    // expanding the column.
    div()
        .id("settings-scroll")
        .size_full()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .child(
            div().w_full().flex().flex_row().justify_center().child(
                div()
                    .flex_1()
                    .max_w(px(680.0))
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .px_5()
                    .py_5()
                    .child(
                        // Heading + version chip on one baseline-
                        // aligned row. Build version is patched into
                        // `CARGO_PKG_VERSION` by the release workflow
                        // before each tagged build; local dev builds
                        // show whatever Cargo.toml carries (0.1.0
                        // until next release), which is itself a
                        // useful "this isn't a published binary" cue.
                        div()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap_2()
                            .child(heading("Settings"))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(theme::text_tertiary())
                                    .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                            ),
                    )
                    .child(speed_box)
                    .child(read_box)
                    .child(window_box)
                    .child(hex_box)
                    .child(log_box)
                    .child(prefs_box),
            ),
        )
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

/// Row used by Settings → Hex viewer: label on the left, a `[-] N px
/// [+]` stepper on the right. Uses tight 24×24 chips instead of the
/// full `action_button_for` shape — the standard CTA button reads
/// out of scale next to a 12px label, and stacking two of them in
/// one row eats half the viewport.
fn font_size_row<FMinus, FPlus>(
    label: &'static str,
    current: f32,
    id_minus: &'static str,
    id_plus: &'static str,
    cx: &mut Context<AppView>,
    on_minus: FMinus,
    on_plus: FPlus,
) -> impl IntoElement
where
    FMinus: Fn(&mut AppView, &mut Context<AppView>) + 'static,
    FPlus: Fn(&mut AppView, &mut Context<AppView>) + 'static,
{
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_1()
                .text_color(theme::text_primary())
                .child(label),
        )
        .child(stepper_button("\u{2212}", id_minus, cx, on_minus))
        .child(
            div()
                .min_w(px(44.0))
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .text_color(theme::text_primary())
                .child(format!("{current:.0} px")),
        )
        .child(stepper_button("+", id_plus, cx, on_plus))
}

/// Compact 24×24 chip used by `font_size_row` for the inline +/-
/// stepper. Same colour as `action_button_for` for visual lineage,
/// just sized to a glyph instead of a CTA label.
fn stepper_button<F>(
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
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.0))
        .h(px(24.0))
        .rounded(px(4.0))
        .text_size(px(13.0))
        .text_color(theme::text_primary())
        .bg(theme::accent_blue())
        .hover(|d| d.bg(theme::accent_blue_hover()))
        .cursor_pointer()
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
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
    .font_family(theme::MONO_FONT)
    .text_size(px(font_size))
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
fn hex_row(
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
        // Group the three register blocks inside a single card so
        // "decoded result" is visually distinct from the pane's
        // heading + body + button stack above. Each SR block keeps
        // its own header (SR1 / SR2 / SR3) inside the card. The
        // Copy button on the card writes a plain-text dump of all
        // three registers to the clipboard for paste-into-issue
        // / share-with-a-coworker workflows.
        let copy_text = format_status_for_copy(&r);
        col = col.child(
            card_with_copy(copy_text, "copy-status", cx)
                .child(status_register_block(
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
                ))
                .child(status_register_block(
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
                ))
                .child(status_register_block(
                    "SR3",
                    r.sr3,
                    r.sr3_present(),
                    &[
                        ("ADP", r.adp().to_string()),
                        ("WPS", r.wps().to_string()),
                        ("DRV", r.drv().to_string()),
                        ("HOLD/RST", r.hold_rst().to_string()),
                    ],
                )),
        );
        // Same gotcha-surfacing note the CLI prints — most common
        // reason a user is on this pane is "writes silently failing"
        // due to BP bits, so call it out directly.
        if r.bp() != 0 || r.sec_or_bp3() {
            col = col.child(armed_warning(
                "SR1 has block-protect bits set: writes and erases to the protected range \
                 will silently fail. Clear BP[2:0] (and SEC/BP3 if set) via WRSR before \
                 programming.",
            ));
        }
    }
    col
}

/// Render the same SR1/SR2/SR3 decode the CLI's `etch341 sr` prints,
/// as a plain-text block suitable for clipboard. Lives in panes.rs
/// so the GUI Copy button doesn't need to round-trip through
/// `ops::print_status` (which writes to stdout).
fn format_status_for_copy(r: &crate::spi::StatusRegisters) -> String {
    let bit = |b: bool| if b { '1' } else { '0' };
    let mut s = String::new();
    s.push_str(&format!("SR1 : 0x{:02X}  (0b{:08b})\n", r.sr1, r.sr1));
    s.push_str(&format!(
        "        WIP={} WEL={} BP={} TB={} SEC/BP3={} SRP0={}\n",
        bit(r.wip()),
        bit(r.wel()),
        r.bp(),
        bit(r.tb()),
        bit(r.sec_or_bp3()),
        bit(r.srp0()),
    ));
    if r.sr2_present() {
        s.push_str(&format!("SR2 : 0x{:02X}  (0b{:08b})\n", r.sr2, r.sr2));
        s.push_str(&format!(
            "        SRP1={} QE={} LB={} CMP={} SUS={}\n",
            bit(r.srp1()),
            bit(r.qe()),
            r.lb(),
            bit(r.cmp()),
            bit(r.sus()),
        ));
    } else {
        s.push_str("SR2 : 0xFF    (chip didn't respond)\n");
    }
    if r.sr3_present() {
        s.push_str(&format!("SR3 : 0x{:02X}  (0b{:08b})\n", r.sr3, r.sr3));
        s.push_str(&format!(
            "        ADP={} WPS={} DRV={} HOLD/RST={}\n",
            bit(r.adp()),
            bit(r.wps()),
            r.drv(),
            bit(r.hold_rst()),
        ));
    } else {
        s.push_str("SR3 : 0xFF    (chip didn't respond)\n");
    }
    s
}

/// One register row inside `status_pane` — section label, raw hex
/// (+ binary), and a wrapped grid of decoded bit name → value pairs.
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
                .font_family(theme::MONO_FONT)
                .text_color(if present {
                    theme::text_primary()
                } else {
                    theme::text_tertiary()
                })
                .child(format!("0x{value:02X}  0b{value:08b}")),
        );
    if !present {
        return div().flex().flex_col().gap_1().child(header).child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_tertiary())
                .whitespace_normal()
                .child(format!(
                    "(chip didn't respond, likely doesn't implement {name})"
                )),
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
        .font_family(theme::MONO_FONT)
        .text_color(theme::text_secondary());
    for (label, v) in bits {
        grid = grid.child(div().child(format!("{label}={v}")));
    }
    div().flex().flex_col().gap_1().child(header).child(grid)
}

/// OTP / security-register pane. Mirrors `etch341 otp read` /
/// `otp erase` / `otp write`: read dumps the three registers as a
/// copyable hexdump card; erase / write target one register behind
/// the same two-stage arm/confirm the Erase and Write panes use.
fn otp_pane(
    regs: Option<&[crate::ops::OtpRegister]>,
    target_register: u8,
    write_path: Option<&Path>,
    erase_armed: bool,
    write_armed: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Security registers",
        "The chip's security registers (Winbond / GigaDevice 0x48 \
         convention: three 256-byte registers) commonly hold serial \
         numbers, MAC addresses, or vendor keys. Read dumps all \
         three. Erase / write target one register — programming only \
         clears bits, so erase first for a clean write. Macronix \
         parts use a different opcode and aren't covered.",
    );
    col = col.child(action_button_for(
        "Read security registers",
        "read-otp",
        cx,
        |this, cx| this.start_read_otp(cx),
    ));
    if let Some(regs) = regs {
        let mut card = card_with_copy(format_otp_for_copy(regs), "copy-otp", cx);
        for reg in regs {
            card = card.child(otp_register_block(reg));
        }
        col = col.child(card);
    }

    // Destructive controls live below a divider, split into two
    // separate outlined boxes so erase and write don't read as one
    // cluster. The target register lives with erase (you pick a
    // register and can wipe it there); write gets its own box. Both
    // act on the same selected register; both are capped at 680px to
    // match the Settings pane's content width.
    col = col.child(otp_divider());

    let selected_idx = (target_register as usize).checked_sub(1);
    let mut target_box = GroupBox::new()
        .id("otp-target-box")
        .outline()
        .max_w(px(680.0))
        .title("Target register")
        .child(
            RadioGroup::horizontal("otp-target")
                .selected_index(selected_idx)
                .on_click(cx.listener(|this: &mut AppView, ix: &usize, _, cx| {
                    this.set_otp_target_register(*ix as u8 + 1, cx);
                }))
                .children(
                    crate::ops::OTP_REGISTER_INDICES
                        .iter()
                        .map(|n| format!("Register {n}")),
                ),
        );
    if erase_armed {
        target_box = target_box.child(armed_warning(
            "Armed: next click erases the selected register to 0xFF.",
        ));
    }
    target_box = target_box.child(armable_button(
        "Erase selected register",
        "Click again to erase",
        "otp-erase",
        erase_armed,
        cx,
        |this, cx| this.arm_or_fire_otp_erase(cx),
    ));

    let mut write_box = GroupBox::new()
        .id("otp-write-box")
        .outline()
        .max_w(px(680.0))
        .title("Write from file")
        .child(otp_file_row(write_path, cx));
    if write_armed && write_path.is_some() {
        write_box = write_box.child(armed_warning(
            "Armed: next click programs the selected register from the file (offset 0).",
        ));
    }
    write_box = write_box.child(armable_button(
        "Write selected register from file",
        "Click again to write",
        "otp-write",
        write_armed,
        cx,
        |this, cx| this.arm_or_fire_otp_write(cx),
    ));

    col = col.child(target_box).child(write_box);
    col
}

/// Hairline divider separating the read output from the destructive
/// modify boxes below it. `my_2` adds margin on top of the pane's
/// 16px flex gap so the read↔modify boundary reads as a real
/// section break rather than just another row.
///
/// `flex_shrink_0` is load-bearing: the pane is a scrolling flex
/// column, and at the default window height the content overflows.
/// Flexbox then shrinks children to fit — a 1px divider with no
/// content shrinks straight to 0 and vanishes (the buttons / boxes
/// survive because they have a min-content height). Pinning shrink
/// to 0 keeps the line at its 1px height regardless of overflow.
fn otp_divider() -> impl IntoElement {
    div()
        .flex_shrink_0()
        .my_2()
        .h(px(1.0))
        .bg(theme::workshop_glass_strong())
}

/// File-path display + Browse button for the OTP write source.
/// Unlike the shared `file_picker_row`, the path sits in a bordered,
/// input-style box so it reads as "the file goes here" rather than
/// loose text floating next to a button.
fn otp_file_row(path: Option<&Path>, cx: &mut Context<AppView>) -> impl IntoElement {
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
                .px_3()
                .py_2()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme::workshop_glass_strong())
                .bg(theme::bench_black())
                .whitespace_normal()
                .text_color(path_color)
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .child(display),
        )
        .child(action_button_for(
            "Browse…",
            "pick-otp",
            cx,
            |this, cx| this.pick_otp_file(cx),
        ))
}

/// Hexdump one register as offset / hex / ASCII text lines, matching
/// the CLI `otp read` layout. Blank (all-0xFF) registers collapse to
/// a single note line instead of 16 identical rows. Shared by the
/// visual block and the clipboard text so the two never drift.
fn otp_hexdump_lines(reg: &crate::ops::OtpRegister) -> Vec<String> {
    if reg.is_blank() {
        return vec!["all 0xFF (blank / unprogrammed)".to_string()];
    }
    reg.data
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let off = reg.addr as usize + i * 16;
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
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
            format!("{off:06X}  {:<47}  |{ascii}|", hex.join(" "))
        })
        .collect()
}

/// One register block inside the OTP card: a small-caps header plus
/// the monospace hexdump lines.
fn otp_register_block(reg: &crate::ops::OtpRegister) -> gpui::Div {
    let header = div()
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(format!("REGISTER {} @ 0x{:06X}", reg.index, reg.addr));
    let mut body = div()
        .flex()
        .flex_col()
        .text_size(px(12.0))
        .font_family(theme::MONO_FONT)
        .text_color(if reg.is_blank() {
            theme::text_tertiary()
        } else {
            theme::text_secondary()
        });
    for line in otp_hexdump_lines(reg) {
        body = body.child(div().whitespace_nowrap().child(line));
    }
    div().flex().flex_col().gap_1().child(header).child(body)
}

/// Plain-text hexdump of all registers for the card's Copy button —
/// matches the CLI `otp read` output so a paste-into-issue reads the
/// same as a terminal session.
fn format_otp_for_copy(regs: &[crate::ops::OtpRegister]) -> String {
    let mut s = String::new();
    for reg in regs {
        s.push_str(&format!("Register {} @ 0x{:06X}:\n", reg.index, reg.addr));
        for line in otp_hexdump_lines(reg) {
            s.push_str("  ");
            s.push_str(&line);
            s.push('\n');
        }
        s.push('\n');
    }
    s
}

/// Build the card that renders a decoded SFDP table: header line,
/// parameter-header list, decoded BFPT, and erase-type table. Used
/// by the Detect pane after a Detect run that picked up SFDP. Lives
/// here (not in the Detect pane render) so the layout is reusable
/// if we ever want a standalone SFDP view back.
fn sfdp_card(parsed: &crate::sfdp::Sfdp, cx: &mut Context<AppView>) -> gpui::Div {
    let mut hdr_grid = mono_block();
    for (i, ph) in parsed.parameter_headers.iter().enumerate() {
        let tag = if ph.id == crate::sfdp::ParameterHeader::BFPT_ID {
            "JEDEC BFPT"
        } else {
            "vendor"
        };
        hdr_grid = hdr_grid.child(div().child(format!(
            "[{i}] id=0x{:04X} ({tag})  rev {}.{}  len={} dwords  @ 0x{:06X}",
            ph.id, ph.major_rev, ph.minor_rev, ph.length_dwords, ph.ptr,
        )));
    }

    let copy_text = format_sfdp_for_copy(parsed);
    let mut output = card_with_copy(copy_text, "copy-sfdp", cx)
        .child(section_block_with_text(
            "SFDP HEADER",
            format!(
                "rev {}.{}  ·  {} parameter header(s)",
                parsed.header.major_rev,
                parsed.header.minor_rev,
                parsed.parameter_headers.len(),
            ),
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("PARAMETER HEADERS"))
                .child(hdr_grid),
        );

    if let Some(bfpt) = &parsed.bfpt {
        let addr = match bfpt.addressing {
            crate::sfdp::Addressing::ThreeByteOnly => "3-byte only",
            crate::sfdp::Addressing::Either => "3- or 4-byte (default 3)",
            crate::sfdp::Addressing::FourByteOnly => "4-byte only",
            crate::sfdp::Addressing::Reserved => "reserved encoding",
        };
        let mut grid = mono_block()
            .child(div().child(format!(
                "size      : {} bytes ({} KB, {} Mbit)",
                bfpt.size_bytes,
                bfpt.size_bytes / 1024,
                bfpt.size_bytes * 8 / 1_000_000,
            )))
            .child(div().child(format!("page size : {} bytes", bfpt.page_size)))
            .child(div().child(format!("address   : {addr}")));
        if bfpt.erase_4k_opcode != 0xFF {
            grid = grid
                .child(div().child(format!("4K erase  : opcode 0x{:02X}", bfpt.erase_4k_opcode)));
        } else {
            grid = grid.child(div().child("4K erase  : not supported".to_string()));
        }
        output = output.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("BFPT"))
                .child(grid),
        );

        let mut etype_grid = mono_block();
        let mut any = false;
        for (i, e) in bfpt.erase_types.iter().enumerate() {
            if e.size_bytes == 0 {
                continue;
            }
            any = true;
            let size = if e.size_bytes >= (1 << 20) {
                format!("{} MB", e.size_bytes >> 20)
            } else if e.size_bytes >= (1 << 10) {
                format!("{} KB", e.size_bytes >> 10)
            } else {
                format!("{} B", e.size_bytes)
            };
            etype_grid = etype_grid.child(div().child(format!(
                "[{i}] opcode 0x{:02X}  {} bytes ({size})",
                e.opcode, e.size_bytes,
            )));
        }
        if !any {
            etype_grid = etype_grid.child(div().child("(none advertised)".to_string()));
        }
        output = output.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label("ERASE TYPES"))
                .child(etype_grid),
        );
    } else {
        output = output.child(
            div()
                .text_color(theme::text_tertiary())
                .child("(BFPT body wasn't reachable inside the 256-byte read window)"),
        );
    }

    output
}

/// Plain-text rendering of the same decoded SFDP view the pane
/// shows visually. Used by the card's Copy button to put the
/// output on the clipboard.
fn format_sfdp_for_copy(parsed: &crate::sfdp::Sfdp) -> String {
    use crate::sfdp::{Addressing, ParameterHeader};
    let mut s = String::new();
    s.push_str(&format!(
        "SFDP rev {}.{}, {} parameter header(s)\n",
        parsed.header.major_rev,
        parsed.header.minor_rev,
        parsed.parameter_headers.len(),
    ));
    for (i, ph) in parsed.parameter_headers.iter().enumerate() {
        let tag = if ph.id == ParameterHeader::BFPT_ID {
            "JEDEC BFPT"
        } else {
            "vendor"
        };
        s.push_str(&format!(
            "  [{i}] id=0x{:04X} ({tag}) rev {}.{} len={} dwords @ 0x{:06X}\n",
            ph.id, ph.major_rev, ph.minor_rev, ph.length_dwords, ph.ptr,
        ));
    }
    if let Some(b) = &parsed.bfpt {
        let addr = match b.addressing {
            Addressing::ThreeByteOnly => "3-byte only",
            Addressing::Either => "3- or 4-byte (default 3)",
            Addressing::FourByteOnly => "4-byte only",
            Addressing::Reserved => "reserved encoding",
        };
        s.push_str("\nBFPT:\n");
        s.push_str(&format!(
            "  size      : {} bytes ({} KB, {} Mbit)\n",
            b.size_bytes,
            b.size_bytes / 1024,
            b.size_bytes * 8 / 1_000_000,
        ));
        s.push_str(&format!("  page size : {} bytes\n", b.page_size));
        s.push_str(&format!("  address   : {addr}\n"));
        if b.erase_4k_opcode != 0xFF {
            s.push_str(&format!(
                "  4K erase  : opcode 0x{:02X}\n",
                b.erase_4k_opcode
            ));
        } else {
            s.push_str("  4K erase  : not supported\n");
        }
        s.push_str("  erase types:\n");
        let mut any = false;
        for (i, e) in b.erase_types.iter().enumerate() {
            if e.size_bytes == 0 {
                continue;
            }
            any = true;
            s.push_str(&format!(
                "    [{i}] opcode 0x{:02X}  {} bytes\n",
                e.opcode, e.size_bytes,
            ));
        }
        if !any {
            s.push_str("    (none advertised)\n");
        }
    }
    s
}

/// Mono-styled flex column used for fixed-width text blocks inside
/// the SFDP card. Pulls font + color + spacing into one place so
/// every section reads with the same rhythm.
fn mono_block() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(12.0))
        .font_family(theme::MONO_FONT)
        .text_color(theme::text_secondary())
}

/// Section label paired with a single line of mono-styled text.
/// Used by the SFDP card's "HEADER" section where the content is
/// a one-liner instead of a multi-row grid.
fn section_block_with_text(label: &'static str, text: String) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_label(label))
        .child(
            div()
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .text_color(theme::text_secondary())
                .child(text),
        )
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

fn detect_pane(
    info: Option<&crate::gui::DetectInfo>,
    sfdp: Option<&crate::sfdp::Sfdp>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = op_pane(
        "Detect",
        "Reads the JEDEC ID, identifies the chip, and pulls the chip's \
         SFDP table if it has one. The other steps detect internally too, \
         so this is optional. Useful as a sanity check before anything \
         destructive.",
    )
    .child(action_button("Detect chip", cx));

    let Some(info) = info else {
        return col;
    };

    // Chip-identification card (JEDEC + chip name + size + source +
    // notes). Always shown after a Detect run; the source field
    // makes it obvious whether the chip name came from the bundled
    // DB or from an SFDP synthesis.
    col = col.child(chip_info_card(info, cx));

    // Second card: the raw decoded SFDP table, if the chip carried
    // one. Rendered for both DB-hit and SFDP-fallback chips so power
    // users can inspect the table regardless of catalogue status.
    if let Some(s) = sfdp {
        col = col.child(sfdp_card(s, cx));
    }
    col
}

fn chip_info_card(info: &crate::gui::DetectInfo, cx: &mut Context<AppView>) -> gpui::Div {
    use crate::gui::ChipSource;
    let mut grid = mono_block().child(div().child(format!("JEDEC : 0x{}", info.jedec)));
    let mut copy_text = format!("JEDEC : 0x{}\n", info.jedec);
    match (&info.source, info.chip.as_ref()) {
        (ChipSource::Database, Some(c)) => {
            grid = grid
                .child(div().child(format!("Chip  : {}", c.name)))
                .child(div().child("Source: chips.toml".to_string()))
                .child(div().child(format!(
                    "Size  : {} KB  ·  page {} B  ·  sector {} B",
                    c.size_kb, c.page_size, c.sector_size,
                )));
            copy_text.push_str(&format!(
                "Chip  : {}\nSource: chips.toml\nSize  : {} KB, page {} B, sector {} B\n",
                c.name, c.size_kb, c.page_size, c.sector_size
            ));
            if !c.notes.is_empty() {
                grid = grid.child(div().child(format!("Notes : {}", c.notes)));
                copy_text.push_str(&format!("Notes : {}\n", c.notes));
            }
        }
        (ChipSource::Sfdp, Some(c)) => {
            grid = grid
                .child(div().child(format!("Chip  : {}", c.name)))
                .child(div().child("Source: SFDP (no chips.toml entry)".to_string()))
                .child(div().child(format!(
                    "Size  : {} KB  ·  page {} B  ·  sector {} B",
                    c.size_kb, c.page_size, c.sector_size,
                )));
            copy_text.push_str(&format!(
                "Chip  : {}\nSource: SFDP (no chips.toml entry)\nSize  : {} KB, page {} B, sector {} B\n",
                c.name, c.size_kb, c.page_size, c.sector_size,
            ));
        }
        (ChipSource::Unknown, _) => {
            grid = grid
                .child(div().child("Chip  : unknown".to_string()))
                .child(div().child("Source: neither chips.toml nor SFDP".to_string()));
            copy_text.push_str("Chip  : unknown\nSource: neither chips.toml nor SFDP\n");
        }
        (ChipSource::NoChip, _) => {
            grid = grid.child(
                div()
                    .text_color(theme::text_tertiary())
                    .child("Chip  : (no chip on the bus)".to_string()),
            );
            copy_text.push_str("Chip  : (no chip on the bus)\n");
        }
        _ => {}
    }
    card_with_copy(copy_text, "copy-detect", cx).child(grid)
}

/// Shared outer shell for the operation panes (Detect / Read / Erase
/// / Write / Verify / Blank / Status / SFDP). Returns a scrollable
/// `flex_col` with the standard pane padding + gap, the heading,
/// and the body paragraph already added — callers chain on the
/// per-pane extras (file picker, armed warning, action button,
/// output cards) via `.child(...)`. Keeps "what a pane looks like"
/// in one place so future tweaks (e.g. changing the body color or
/// the gap between rows) land here instead of in eight
/// near-identical copies.
///
/// `id` is derived from the heading text so each pane gets a
/// distinct scroll handle — gpui requires a stable identifier on
/// any interactive (here, scrollable) element. The container is
/// `size_full()` + `min_h(0)` so flex shrinks it to the resizable
/// pane area rather than expanding to its content height, which
/// is what lets `overflow_y_scroll` actually clip the content
/// instead of growing the parent.
fn op_pane(heading_text: &'static str, body_text: &'static str) -> gpui::Stateful<gpui::Div> {
    div()
        .id(heading_text)
        .size_full()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_5()
        .child(heading(heading_text))
        .child(body(body_text))
}

/// Visually-distinct container for pane output (Status register
/// blocks, SFDP decoded fields, etc). The slightly lighter glass
/// background + hairline border sets "this is the result of an
/// operation" apart from the surrounding pane chrome so the user's
/// eye lands on it without having to track what's heading vs
/// description vs output. Callers append the actual content
/// children via `.child(...)`.
fn card() -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .rounded(px(8.0))
        .bg(theme::workshop_glass())
        .border_1()
        .border_color(theme::workshop_glass_strong())
}

/// `card()` with a small "Copy" pill button anchored to the top
/// right. Clicking the button writes `copy_text` to the system
/// clipboard. Used by output panes (Status registers, SFDP) so
/// users can share / paste the decoded output without retyping —
/// GPUI's `div().child("text")` doesn't natively support
/// cursor-based text selection, so a copy-all button is the
/// pragmatic substitute. The first child of the returned div is
/// the button row; callers chain the actual content below it
/// via `.child(...)`.
fn card_with_copy(
    copy_text: String,
    copy_id: &'static str,
    cx: &mut Context<AppView>,
) -> gpui::Div {
    card().child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .child(copy_button(copy_text, copy_id, cx)),
    )
}

/// Small inline "Copy" pill button. Click writes `text` to the
/// clipboard and pushes a brief confirmation to the activity log.
fn copy_button(text: String, id: &'static str, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .id(id)
        .px_2()
        .py_0p5()
        .rounded(px(4.0))
        .text_size(px(11.0))
        .text_color(theme::text_secondary())
        .bg(theme::workshop_glass_strong())
        .cursor_pointer()
        .hover(|d| d.text_color(theme::text_primary()))
        .child("Copy")
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                let payload = text.clone();
                let len = payload.len();
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(payload));
                this.push_log(format!("Copied {len} chars to clipboard"));
            }),
        )
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
    styled_button(id).bg(bg).child(label).on_click(cx.listener(
        move |this: &mut AppView, _: &ClickEvent, _, cx| {
            on_click(this, cx);
        },
    ))
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
