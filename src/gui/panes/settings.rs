//! Settings pane: clock, appearance, fonts, update check, window.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn settings_pane(
    current_khz: u32,
    restore_window_bounds: bool,
    read_output_dir: Option<&Path>,
    prefs_path: Option<&Path>,
    hex_font_size: f32,
    strings_font_size: f32,
    timestamp_local: bool,
    update_check_enabled: bool,
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

    // Appearance section: accent-color swatches. The selected
    // swatch is read straight from the theme global (set_accent
    // updates it before the re-render), so it needn't be threaded
    // through PaneInputs.
    let current_accent = theme::accent_hex();
    let mut swatches = div().flex().flex_row().flex_wrap().gap_3();
    for &(name, hex, _) in theme::ACCENT_PRESETS {
        swatches = swatches.child(accent_swatch(name, hex, hex == current_accent, cx));
    }
    let appearance_box = GroupBox::new()
        .id("settings-appearance")
        .outline()
        .title("Appearance")
        .child(body(
            "Accent color used across buttons, selections, toggles, and \
             controls. Click a swatch.",
        ))
        .child(swatches);

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

    // Updates section: the on/off toggle, a version status line, and
    // View-release / Check-now buttons. The available update (if any)
    // is read straight from the updater global.
    let available = crate::gui::updater::available(cx);
    let mut updates_box = GroupBox::new()
        .id("settings-updates")
        .outline()
        .title("Updates")
        .child(body(
            "Checks the project's GitHub releases for a newer version when \
             etch341 launches, and marks the Settings sidebar item with a \
             dot if one exists. Detection only — it never downloads or \
             installs anything.",
        ))
        .child(toggle_row(
            "Check for updates on launch",
            "toggle-update-check",
            update_check_enabled,
            cx,
            // Flip: new enabled = !current = the current
            // `disable_update_check` value.
            |this, cx| this.set_update_check_enabled(this.prefs.disable_update_check, cx),
        ));
    updates_box = updates_box.child(match &available {
        Some(up) => div()
            .text_color(theme::text_primary())
            .whitespace_normal()
            .child(format!(
                "Version {} is available — you have v{}.",
                up.version,
                env!("CARGO_PKG_VERSION")
            )),
        None => div()
            .text_size(px(12.0))
            .text_color(theme::text_tertiary())
            .child(format!("Installed: v{}", env!("CARGO_PKG_VERSION"))),
    });
    let mut update_buttons = div().flex().flex_row().gap_3();
    if available.is_some() {
        update_buttons = update_buttons.child(action_button_for(
            "View release",
            "open-release",
            cx,
            |this, cx| this.open_release_page(cx),
        ));
    }
    update_buttons = update_buttons.child(action_button_for(
        "Check now",
        "check-updates",
        cx,
        |this, cx| this.check_for_updates_now(cx),
    ));
    updates_box = updates_box.child(update_buttons);

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
                    .child(appearance_box)
                    .child(hex_box)
                    .child(log_box)
                    .child(updates_box)
                    .child(prefs_box),
            ),
        )
}

/// One accent-color swatch in Settings → Appearance: a filled circle
/// that gains a white halo ring (a 2px gap + 2px border) when it's
/// the active accent. `name` is the tooltip and the element id.
pub(super) fn accent_swatch(
    name: &'static str,
    hex: u32,
    selected: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    div()
        .id(name)
        .p_0p5()
        .rounded_full()
        .border_2()
        .border_color(if selected {
            theme::text_primary()
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .child(
            div()
                .w(px(22.0))
                .h(px(22.0))
                .rounded_full()
                .bg(theme::swatch_color(hex)),
        )
        .tooltip(move |window, cx| Tooltip::new(name).build(window, cx))
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                this.set_accent(hex, cx);
            }),
        )
}

/// Switch-style row: label on the left, a small pill on the right
/// that fills when `active`. Mirrors `speed_row`'s shape (clickable
/// the whole way across, hover tint, accent-blue when on) so the
/// settings pane looks consistent. Used for boolean prefs.
pub(super) fn toggle_row<F>(
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
        theme::accent()
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
pub(super) fn font_size_row<FMinus, FPlus>(
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
pub(super) fn stepper_button<F>(
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
        .text_color(theme::accent_foreground())
        .bg(theme::accent())
        .hover(|d| d.bg(theme::accent_hover()))
        .cursor_pointer()
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                on_click(this, cx);
            }),
        )
}
