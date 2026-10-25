//! Settings pane on GPUI Kit's `Settings` layout: a searchable page
//! sidebar, one box per setting. Fields read and write `AppView`
//! through its entity handle, so the pane takes no prefs arguments.

// The parent module is this submodule's prelude (see panes.rs):
// `use super::*` pulls its imports + shared widget helpers in.
use super::*;
use gpui::App;
use gpui_kit::assets::IconName;
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::setting::{
    NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
};

pub(super) fn settings_pane(
    prefs_path: Option<&Path>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let view = cx.entity();
    Settings::new("etch-settings")
        .with_group_variant(GroupBoxVariant::Outline)
        .pages([
            programmer_page(&view),
            appearance_page(&view),
            general_page(&view, prefs_path.map(Path::to_path_buf)),
        ])
}

/// Read a value off `AppView` for a field getter.
fn read<T>(
    view: &Entity<AppView>,
    f: impl Fn(&AppView) -> T + 'static,
) -> impl Fn(&App) -> T + 'static {
    let view = view.clone();
    move |cx| f(view.read(cx))
}

/// `(id, label, action)` for `path_row`'s button.
type PathButton = (
    &'static str,
    &'static str,
    fn(&mut AppView, &mut Context<AppView>),
);

/// Mono path text + a button, for the folder / file rows.
fn path_row(text: String, button: Option<PathButton>, view: Entity<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .w_full()
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(12.0))
                .font_family(theme::MONO_FONT)
                .text_color(theme::text_secondary())
                .whitespace_normal()
                .child(text),
        )
        .when_some(button, move |row, (id, label, action)| {
            row.child(
                Button::new(id)
                    .label(label)
                    .small()
                    .on_click(move |_, _, cx| {
                        view.update(cx, action);
                    }),
            )
        })
}

fn programmer_page(view: &Entity<AppView>) -> SettingPage {
    // Matches `ch341::SUPPORTED_SPEEDS_KHZ` — the set the CH341A
    // accepts via the standard I²C-stream set-speed command.
    let speeds: Vec<(SharedString, SharedString)> = [
        (20, "20 kHz"),
        (100, "100 kHz"),
        (400, "400 kHz"),
        (750, "750 kHz (recommended)"),
    ]
    .into_iter()
    .map(|(k, label)| (k.to_string().into(), label.into()))
    .collect();
    let set_view = view.clone();
    let dir_view = view.clone();

    SettingPage::new("Programmer")
        .icon(IconName::Cpu)
        .default_open(true)
        .group(
            SettingGroup::new().item(
                SettingItem::new(
                    "SPI clock speed",
                    SettingField::dropdown(
                        speeds,
                        read(view, |a| a.prefs.spi_speed_khz.to_string().into()),
                        move |v: SharedString, cx| {
                            if let Ok(khz) = v.parse() {
                                set_view.update(cx, |this, cx| this.set_spi_speed(khz, cx));
                            }
                        },
                    ),
                )
                .description(
                    "Clock rate for every SPI op (Detect / Read / Erase / Write / \
                     Verify / Blank-check). 750 kHz is the fastest reliable rate; \
                     20 kHz is the most signal-tolerant. Saved immediately; the \
                     next op picks up the new value when it opens the CH341A.",
                ),
            ),
        )
        .group(
            SettingGroup::new().item(
                SettingItem::new(
                    "Read save location",
                    SettingField::render(move |_, _, cx| {
                        let text = dir_view
                            .read(cx)
                            .prefs
                            .read_output_dir
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| {
                                std::env::var("HOME")
                                    .map(|h| format!("{h} (default)"))
                                    .unwrap_or_else(|_| "(default: $HOME, not set)".to_string())
                            });
                        path_row(
                            text,
                            Some(("pick-read-output", "Browse…", AppView::pick_read_output_dir)),
                            dir_view.clone(),
                        )
                    }),
                )
                .description(
                    "Where the Read pane saves chip dumps. Defaults to your home \
                     directory; pick a folder to override.",
                )
                .layout(gpui::Axis::Vertical)
                .keywords(["read", "save", "folder", "dump", "location"]),
            ),
        )
}

fn appearance_page(view: &Entity<AppView>) -> SettingPage {
    let swatch_view = view.clone();
    let (hex_set, strings_set, ts_set) = (view.clone(), view.clone(), view.clone());
    let font_opts = NumberFieldOptions {
        min: crate::prefs::HEX_FONT_MIN as f64,
        max: crate::prefs::HEX_FONT_MAX as f64,
        step: 1.0,
    };

    SettingPage::new("Appearance")
        .icon(IconName::Palette)
        .resettable(true)
        .group(
            SettingGroup::new().item(
                SettingItem::new(
                    "Accent color",
                    SettingField::render(move |_, _, _| {
                        let current = theme::accent_hex();
                        let view = swatch_view.clone();
                        div().flex().flex_row().flex_wrap().gap_3().children(
                            theme::ACCENT_PRESETS.iter().map(move |&(name, hex, _)| {
                                accent_swatch(name, hex, hex == current, view.clone())
                            }),
                        )
                    }),
                )
                .description("Used across buttons, selections, toggles, and controls.")
                .layout(gpui::Axis::Vertical),
            ),
        )
        .group(
            SettingGroup::new()
                .title("Hex viewer")
                .description(
                    "Font sizes inside the Hex pane. Also adjustable on the fly \
                     with Cmd +/- and Cmd 0 on macOS, or Ctrl on Windows / Linux.",
                )
                .item(SettingItem::new(
                    "Hex view font size",
                    SettingField::number_input(
                        font_opts.clone(),
                        read(view, |a| a.prefs.hex_font_size as f64),
                        move |v, cx| {
                            hex_set.update(cx, |this, cx| {
                                this.nudge_hex_font(v as f32 - this.prefs.hex_font_size, cx)
                            })
                        },
                    )
                    .default_value(crate::prefs::HEX_FONT_DEFAULT as f64),
                ))
                .item(SettingItem::new(
                    "Strings view font size",
                    SettingField::number_input(
                        font_opts,
                        read(view, |a| a.prefs.strings_font_size as f64),
                        move |v, cx| {
                            strings_set.update(cx, |this, cx| {
                                this.nudge_strings_font(v as f32 - this.prefs.strings_font_size, cx)
                            })
                        },
                    )
                    .default_value(crate::prefs::HEX_FONT_DEFAULT as f64),
                )),
        )
        .group(
            SettingGroup::new().item(
                SettingItem::new(
                    "Show local time in the activity log",
                    SettingField::switch(read(view, |a| a.prefs.timestamp_local), move |on, cx| {
                        ts_set.update(cx, |this, cx| this.set_timestamp_local(on, cx))
                    })
                    .default_value(false),
                )
                .description(
                    "Log entries always store UTC; this only changes how they're \
                     displayed. Both existing and new lines re-format on flip.",
                ),
            ),
        )
}

fn general_page(view: &Entity<AppView>, prefs_path: Option<std::path::PathBuf>) -> SettingPage {
    let (bounds_set, update_set, status_view, prefs_view) =
        (view.clone(), view.clone(), view.clone(), view.clone());

    SettingPage::new("General")
        .icon(IconName::Settings)
        .group(
            SettingGroup::new().title("Window").item(
                SettingItem::new(
                    "Restore window position on startup",
                    SettingField::switch(
                        read(view, |a| a.prefs.restore_window_bounds),
                        move |on, cx| {
                            bounds_set.update(cx, |this, cx| {
                                if this.prefs.restore_window_bounds != on {
                                    this.toggle_restore_window_bounds(cx)
                                }
                            })
                        },
                    ),
                )
                .description(
                    "Saves the window's position and size on close and restores \
                     them on the next launch. Off: the window opens centered at \
                     1200 × 800.",
                ),
            ),
        )
        .group(
            SettingGroup::new()
                .title("Updates")
                .item(
                    SettingItem::new(
                        "Check for updates on launch",
                        SettingField::switch(
                            read(view, |a| !a.prefs.disable_update_check),
                            move |on, cx| {
                                update_set
                                    .update(cx, |this, cx| this.set_update_check_enabled(on, cx))
                            },
                        ),
                    )
                    .description(
                        "Checks the project's GitHub releases for a newer version \
                         when etch341 launches, and marks the Settings sidebar item \
                         with a dot if one exists. Detection only — it never \
                         downloads or installs anything.",
                    ),
                )
                .item(SettingItem::render(move |_, _, cx| {
                    let available = crate::gui::updater::available(cx);
                    let status = match &available {
                        Some(up) => format!(
                            "Version {} is available — you have v{}.",
                            up.version,
                            env!("CARGO_PKG_VERSION")
                        ),
                        None => format!("Installed: v{}", env!("CARGO_PKG_VERSION")),
                    };
                    let (release_view, check_view) = (status_view.clone(), status_view.clone());
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_3()
                        .w_full()
                        .child(div().flex_1().whitespace_normal().child(status))
                        .when(available.is_some(), |row| {
                            row.child(
                                Button::new("open-release")
                                    .label("View release")
                                    .small()
                                    .primary()
                                    .on_click(move |_, _, cx| {
                                        release_view
                                            .update(cx, |this, cx| this.open_release_page(cx))
                                    }),
                            )
                        })
                        .child(
                            Button::new("check-updates")
                                .label("Check now")
                                .small()
                                .on_click(move |_, _, cx| {
                                    check_view.update(cx, |this, cx| this.check_for_updates_now(cx))
                                }),
                        )
                })),
        )
        .group(
            SettingGroup::new().title("Preferences file").item(
                SettingItem::render(move |_, _, _| {
                    let text = prefs_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(unable to determine; $HOME not set)".to_string());
                    // Button hidden when there's no real path — nothing
                    // useful to open if $HOME wasn't set.
                    let button = prefs_path.is_some().then_some((
                        "open-prefs-folder",
                        "Open folder",
                        AppView::open_prefs_folder as fn(&mut AppView, &mut Context<AppView>),
                    ));
                    path_row(text, button, prefs_view.clone())
                })
                .keywords(["preferences", "prefs", "file", "toml"]),
            ),
        )
}

/// One accent-color swatch in Settings → Appearance: a filled circle
/// that gains a white halo ring (a 2px gap + 2px border) when it's
/// the active accent. `name` is the tooltip and the element id.
fn accent_swatch(
    name: &'static str,
    hex: u32,
    selected: bool,
    view: Entity<AppView>,
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
        .on_click(move |_, _, cx| view.update(cx, |this, cx| this.set_accent(hex, cx)))
}
