//! BIOS Setup explorer pane: pick a flash image, list its Setup
//! options as label → current value → choices, filterable by label.
//! The read-only GUI twin of `etch341 bios settings`.

// The parent module is this submodule's prelude (see panes.rs).
use super::*;

/// Fixed row height so `uniform_list` can virtualise the (often
/// thousands of) settings without measuring each row.
const ROW_H: f32 = 30.0;

/// Shared column widths so the sticky header and the data rows line up.
const MARKER_W: f32 = 20.0;
const VALUE_W: f32 = 200.0;
const SOURCE_W: f32 = 190.0;
/// Horizontal inset applied identically to the header and every row.
const ROW_PX: f32 = 12.0;
use crate::gui::BIOS_BOOT_VIEW as BOOT_VIEW;

/// Collapse a label to a single line: some HII titles (esp. Insyde)
/// resolve to multi-line message strings, and an embedded `\n` breaks
/// out of a fixed-height row and overlaps its neighbours.
fn oneline(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[allow(clippy::too_many_arguments)]
pub(super) fn bios_pane(
    path: Option<&Path>,
    settings: Option<Arc<Vec<crate::uefi::Setting>>>,
    tree: Option<Arc<Vec<crate::uefi::FormNode>>>,
    boot: Option<Arc<Vec<crate::uefi::BootEntry>>>,
    bios_id: Option<&crate::uefi::BiosId>,
    bios_ifd: Option<&crate::ifd::Ifd>,
    selected_form: Option<&str>,
    changed_only: bool,
    scroll: UniformListScrollHandle,
    nav: &Entity<gpui_kit::component::tree::TreeState>,
    nav_rows: Arc<Vec<(Option<String>, usize)>>,
    search_term: &str,
    search_state: &Entity<InputState>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let mut col = div()
        .flex_1()
        .min_h(px(0.0))
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .py_5()
        .child(heading("BIOS explorer"))
        .child(body(
            "Load a UEFI BIOS dump to browse its Setup options — the label, \
             its current value, and the choices behind each variable byte. \
             Parses firmware volumes → IFR forms → HII strings and joins them \
             against the NVRAM store. Read-only. Pick a menu page on the left \
             to drill in; a ✷ marks options the firmware may hide or lock.",
        ))
        .child(
            GroupBox::new()
                .id("bios-file-box")
                .outline()
                .max_w(px(680.0))
                .title("BIOS image to explore")
                .child(bordered_file_row(path, "pick-bios", cx, |this, cx| {
                    this.pick_bios_file(cx)
                })),
        )
        .when_some(bios_id.filter(|id| !id.is_empty()), |c, id| {
            c.child(bios_id_line(id))
        })
        .when_some(bios_ifd, |c, ifd| c.child(ifd_box(ifd)));

    let Some(settings) = settings else {
        // Nothing loaded yet — the file row above is the whole pane.
        return col;
    };
    if settings.is_empty() && bios_ifd.is_none() {
        // Parsed fine but found no Setup forms: not a UEFI image (a
        // router bootloader, a VBIOS, …). Say so rather than show an
        // empty table.
        return col.child(body(
            "No UEFI Setup settings found in this image. It doesn't look \
             like a UEFI BIOS (no firmware volumes with Setup forms).",
        ));
    }

    // Search overrides navigation (matches across every form); otherwise
    // the selected form scopes the list, and with neither we show all.
    let needle = search_term.to_lowercase();
    let searching = !needle.is_empty();
    let visible: Arc<Vec<usize>> = Arc::new(
        settings
            .iter()
            .enumerate()
            .filter(|(_, s)| !changed_only || s.changed == Some(true))
            .filter(|(_, s)| {
                if searching {
                    s.name.to_lowercase().contains(&needle)
                } else if let Some(f) = selected_form {
                    s.form == f
                } else {
                    true
                }
            })
            .map(|(i, _)| i)
            .collect(),
    );

    let count_line = format!(
        "{} of {} settings{}",
        visible.len(),
        settings.len(),
        if searching {
            format!(" matching “{search_term}”")
        } else if let Some(f) = selected_form {
            format!(" in {f}")
        } else {
            String::new()
        },
    );
    let items = build_items(&settings, &visible);

    col = col
        // Row wrapper so the Input's flex_1 grows horizontally; placing
        // flex_1 directly in this column would stretch it vertically and
        // shove the list down the pane.
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(div().flex_1().child(Input::new(search_state)))
                .child(changed_toggle(changed_only, cx))
                .child(action_pill(
                    "Compare with…",
                    "bios-compare",
                    cx,
                    |this, cx| this.pick_bios_compare(cx),
                ))
                .child(action_pill(
                    "Export JSON…",
                    "bios-export",
                    cx,
                    |this, cx| this.export_bios_json(cx),
                )),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme::text_tertiary())
                        .child(count_line),
                )
                .child(legend()),
        )
        .child(
            // Split: menu navigator on the left, settings (or the boot
            // list) on the right.
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_row()
                .gap_3()
                .when(tree.is_some_and(|t| !t.is_empty()), |row| {
                    row.child(navigator(nav, nav_rows))
                })
                .child(if selected_form == Some(BOOT_VIEW) {
                    boot_panel(boot).into_any_element()
                } else {
                    settings_list(settings, items, scroll).into_any_element()
                }),
        );

    col
}

/// Left navigator: GPUI Kit `Tree` over "Boot order", "All settings"
/// and the collapsible menu tree. Selecting a row scopes the settings
/// list (AppView observes the tree state). Each row shows the form's
/// own setting count.
fn navigator(
    nav: &Entity<gpui_kit::component::tree::TreeState>,
    rows: Arc<Vec<(Option<String>, usize)>>,
) -> impl IntoElement {
    use gpui_kit::assets::IconName;
    use gpui_kit::component::list::ListItem;
    use gpui_kit::component::tree::Tree;
    use gpui_kit::component::{Icon, Sizable as _};

    div()
        .w(px(280.0))
        .flex_shrink_0()
        .border_1()
        .border_color(theme::workshop_glass_strong())
        .rounded(px(6.0))
        .bg(theme::bench_black())
        .overflow_hidden()
        .py_1()
        .child(Tree::new(nav, move |ix, entry, _selected, _, _| {
            let count = entry
                .item()
                .id
                .parse::<usize>()
                .ok()
                .and_then(|i| rows.get(i))
                .map_or(0, |(_, c)| *c);
            let chevron = entry.is_folder().then(|| {
                Icon::new(if entry.is_expanded() {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall()
                .text_color(theme::text_tertiary())
            });
            ListItem::new(ix)
                .pl(px(8.0 + entry.depth() as f32 * 14.0))
                .pr(px(8.0))
                .text_size(px(12.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .w_full()
                        .child(div().w(px(12.0)).flex_shrink_0().children(chevron))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .child(oneline(&entry.item().label)),
                        )
                        .when(count > 0, |row| {
                            row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.0))
                                    .text_color(theme::text_tertiary())
                                    .child(count.to_string()),
                            )
                        }),
                )
        }))
}

/// The boot-order view shown when the navigator's "Boot order" entry
/// is selected: the decoded `BootOrder` in menu order.
fn boot_panel(boot: Option<Arc<Vec<crate::uefi::BootEntry>>>) -> impl IntoElement {
    let entries = boot.map(|b| (*b).clone()).unwrap_or_default();
    let mut inner = div().flex().flex_col().gap_1().px_3().py_2();
    for (i, e) in entries.iter().enumerate() {
        let color = if e.active {
            theme::text_primary()
        } else {
            theme::text_tertiary()
        };
        let mut row = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .h(px(28.0))
            .whitespace_nowrap()
            .child(
                div()
                    .w(px(24.0))
                    .flex_shrink_0()
                    .text_color(theme::text_tertiary())
                    .child(format!("{}.", i + 1)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_color(color)
                    .child(e.description.clone()),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .font_family(theme::MONO_FONT)
                    .text_size(px(11.0))
                    .text_color(theme::text_tertiary())
                    .child(e.slot.clone()),
            );
        if !e.active {
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(11.0))
                    .text_color(theme::caution_red())
                    .child("inactive"),
            );
        }
        inner = inner.child(row);
    }

    div()
        .flex_1()
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme::workshop_glass_strong())
        .rounded(px(6.0))
        .bg(theme::bench_black())
        .overflow_hidden()
        .child(
            div()
                .px_3()
                .py_2()
                .bg(theme::workshop_glass())
                .border_b_1()
                .border_color(theme::workshop_glass_strong())
                .text_size(px(11.0))
                .text_color(theme::text_tertiary())
                .child("BOOT ORDER"),
        )
        .child(
            div()
                .id("bios-boot-list")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .child(inner),
        )
}

/// Colour key for the value column, shown beside the count.
fn legend() -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .flex_shrink_0()
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(legend_dot(theme::warning_amber(), "changed"))
        .child(legend_dot(theme::success_green(), "set"))
        .child(legend_dot(theme::text_tertiary(), "not set"))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .child(div().text_color(theme::warning_amber()).child("✷"))
                .child("conditional"),
        )
}

fn legend_dot(color: gpui::Hsla, label: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(div().size(px(8.0)).flex_shrink_0().rounded_full().bg(color))
        .child(label)
}

/// Compact firmware-identity line under the file box.
fn bios_id_line(id: &crate::uefi::BiosId) -> impl IntoElement + use<> {
    let parts: Vec<String> = [id.vendor.clone(), id.fid.clone(), id.platform.clone()]
        .into_iter()
        .flatten()
        .collect();
    div()
        .text_size(px(12.0))
        .text_color(theme::text_secondary())
        .child(parts.join("   ·   "))
}

/// Compact Intel Flash Descriptor strip: region map + lock summary.
/// Mirrors the `etch341 ifd` CLI output for GUI/CLI parity.
fn ifd_box(ifd: &crate::ifd::Ifd) -> impl IntoElement + use<> {
    let mut rows = div().flex().flex_col().gap_1();
    for r in &ifd.regions {
        let locked = match r.index {
            0 => Some(!ifd.bios_can_write(0)), // Descriptor
            2 => Some(!ifd.bios_can_write(2)), // Intel ME
            _ => None,
        };
        let tag = match locked {
            Some(true) => "  🔒 locked",
            Some(false) => "  ⚠ host-writable",
            None => "",
        };
        rows = rows.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .text_size(px(12.0))
                .text_color(theme::text_secondary())
                .child(div().w(px(120.0)).flex_shrink_0().child(r.name))
                .child(
                    div()
                        .w(px(180.0))
                        .flex_shrink_0()
                        .text_color(theme::text_tertiary())
                        .child(format!("{:#08x}–{:#08x}", r.base, r.limit)),
                )
                .child(format!("{}{tag}", fmt_bytes(r.size() as u64))),
        );
    }
    let subtitle = match ifd.density_bytes {
        Some(d) => format!("Flash layout (IFD) — {} chip", fmt_bytes(d)),
        None => "Flash layout (IFD)".to_string(),
    };
    GroupBox::new()
        .id("bios-ifd-box")
        .outline()
        .max_w(px(680.0))
        .title(subtitle)
        .child(rows)
}

/// Human byte size, matching the CLI's `fmt_bytes` (binary units, "MB").
fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{} MB", n >> 20)
    } else if n >= 1 << 10 {
        format!("{} KB", n >> 10)
    } else {
        format!("{n} B")
    }
}

/// The "Changed only" filter pill next to the search box.
fn changed_toggle(active: bool, cx: &mut Context<AppView>) -> impl IntoElement {
    let (bg, fg) = if active {
        (theme::accent_tint(), theme::text_primary())
    } else {
        (theme::workshop_glass(), theme::text_secondary())
    };
    div()
        .id("bios-changed-toggle")
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_3()
        .py_1p5()
        .rounded(px(6.0))
        .cursor_pointer()
        .bg(bg)
        .text_color(fg)
        .text_size(px(12.0))
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .child(if active { "☑" } else { "☐" })
        .child("Changed only")
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_bios_changed_only(cx)))
}

/// A secondary glass action button (Export / Compare).
fn action_pill<F>(
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
        .flex_shrink_0()
        .px_3()
        .py_1p5()
        .rounded(px(6.0))
        .cursor_pointer()
        .bg(theme::workshop_glass())
        .text_color(theme::text_secondary())
        .text_size(px(12.0))
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .child(label)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
}

/// A rendered list entry: either a form section header or a setting.
enum Item {
    FormHeader(String),
    Setting(usize),
}

/// Flatten the visible settings into header+row items, inserting a
/// form header each time the (already form-sorted) form changes.
fn build_items(settings: &[crate::uefi::Setting], visible: &[usize]) -> Arc<Vec<Item>> {
    let mut items = Vec::new();
    let mut last_form: Option<&str> = None;
    for &idx in visible {
        let form = if settings[idx].form.is_empty() {
            "(uncategorised)"
        } else {
            settings[idx].form.as_str()
        };
        if last_form != Some(form) {
            items.push(Item::FormHeader(form.to_string()));
            last_form = Some(form);
        }
        items.push(Item::Setting(idx));
    }
    Arc::new(items)
}

/// Word-wrap `text` to at most `width` columns per line, preserving
/// any newlines already present. Keeps the tooltip from overflowing
/// the window.
fn wrap(text: &str, width: usize) -> String {
    let mut out = String::new();
    for (li, line) in text.split('\n').enumerate() {
        if li > 0 {
            out.push('\n');
        }
        let mut col = 0;
        for (wi, word) in line.split_whitespace().enumerate() {
            let sep = if wi == 0 { 0 } else { 1 };
            if wi > 0 && col + sep + word.len() > width {
                out.push('\n');
                col = 0;
            } else if wi > 0 {
                out.push(' ');
                col += 1;
            }
            out.push_str(word);
            col += word.len();
        }
    }
    out
}

/// Bordered list box: a fixed header row on top, then the virtualised
/// rows scrolling beneath it. The header stays put because it's a
/// sibling of `uniform_list`, not part of its scrolled content.
fn settings_list(
    settings: Arc<Vec<crate::uefi::Setting>>,
    items: Arc<Vec<Item>>,
    scroll: UniformListScrollHandle,
) -> impl IntoElement {
    let count = items.len();
    div()
        .flex_1()
        .min_h(px(0.0))
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme::workshop_glass_strong())
        .rounded(px(6.0))
        .bg(theme::bench_black())
        .overflow_hidden()
        .child(header_row())
        .child(
            uniform_list("bios-settings-list", count, move |range, _, _| {
                range
                    .map(|i| match &items[i] {
                        Item::FormHeader(name) => form_header(name).into_any_element(),
                        Item::Setting(idx) => setting_row(&settings[*idx], i).into_any_element(),
                    })
                    .collect()
            })
            .flex_1()
            .min_h(px(0.0))
            .py_1()
            .track_scroll(&scroll),
        )
}

/// An inline form section header (the BIOS menu page name), scrolling
/// with the rows beneath the fixed column header.
fn form_header(name: &str) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .h(px(ROW_H))
        .px(px(ROW_PX))
        .bg(theme::workshop_glass_strong())
        .text_color(theme::accent())
        .text_size(px(12.0))
        .child(name.to_string())
}

/// Column titles, styled distinctly and pinned above the scroll.
fn header_row() -> impl IntoElement {
    let cell = |w: f32, label: &str| div().w(px(w)).flex_shrink_0().child(label.to_string());
    div()
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .gap_3()
        .px(px(ROW_PX))
        .py_2()
        .bg(theme::workshop_glass())
        .border_b_1()
        .border_color(theme::workshop_glass_strong())
        .text_size(px(11.0))
        .text_color(theme::text_tertiary())
        .child(div().flex_1().min_w(px(0.0)).child("SETTING"))
        .child(cell(MARKER_W, ""))
        .child(cell(VALUE_W, "VALUE"))
        .child(cell(SOURCE_W, "VARIABLE + OFFSET"))
}

/// One setting row: label · current value · source (variable+offset),
/// with a hover tooltip carrying the help text and the full choice
/// list. Single line at a fixed height for the virtualised list.
fn setting_row(s: &crate::uefi::Setting, virtual_i: usize) -> impl IntoElement + use<> {
    let value = match (&s.value_label, s.value) {
        (Some(label), Some(v)) => format!("{label} (0x{v:x})"),
        (Some(label), None) => label.clone(), // string / ordered list
        (None, Some(v)) => format!("0x{v:x}"),
        (None, None) => "—".to_string(),
    };
    let changed = s.changed == Some(true);
    // Amber flags a value changed from default; green = a resolved
    // current value; dim = not set.
    let value_color = if changed {
        theme::warning_amber()
    } else if s.value.is_some() || s.value_label.is_some() {
        theme::success_green()
    } else {
        theme::text_tertiary()
    };

    // Tooltip: help line (if any), the choices, and — when changed —
    // the default it differs from. Many HII help strings are blank or
    // whitespace-only, so trim to avoid empty leading lines.
    let mut tip = s.help.trim().to_string();
    let choices: Vec<&str> = s
        .options
        .iter()
        .map(|(_, l)| l.as_str())
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !choices.is_empty() {
        if !tip.is_empty() {
            tip.push_str("\n\n");
        }
        tip.push_str(&format!("Choices: {}", choices.join(" / ")));
    }
    if let Some((min, max, step)) = s.range {
        if !tip.is_empty() {
            tip.push_str("\n\n");
        }
        tip.push_str(&format!("Range: {min}–{max}, step {step}"));
    }
    if changed {
        let def = match (&s.default_label, s.default_value) {
            (Some(d), _) => d.clone(),
            (None, Some(d)) => format!("0x{d:x}"),
            _ => "?".to_string(),
        };
        if !tip.is_empty() {
            tip.push_str("\n\n");
        }
        tip.push_str(&format!("Changed from default: {def}"));
    }
    let source = format!("{}+0x{:04x}", s.varstore, s.offset);

    // Fixed-width columns in a full-width row so every row's value and
    // source line up into clean vertical columns regardless of label
    // length or whether the conditional marker is present. The marker
    // gets its own fixed slot (empty when not conditional) so it never
    // shifts the value column.
    let mut row = div()
        .id(("bios-row", virtual_i))
        .flex()
        .flex_row()
        .items_center()
        .w_full()
        .gap_3()
        .h(px(ROW_H))
        .px(px(ROW_PX))
        .whitespace_nowrap()
        // Label — flexes to fill, truncates when long.
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_color(theme::text_primary())
                .child(oneline(&s.name)),
        )
        // Conditional marker — fixed slot, always present so it never
        // shifts the value column.
        .child(
            div()
                .w(px(MARKER_W))
                .flex_shrink_0()
                .text_color(theme::warning_amber())
                .child(if s.conditional { "✷" } else { "" }),
        )
        // Current value.
        .child(
            div()
                .w(px(VALUE_W))
                .flex_shrink_0()
                .overflow_hidden()
                .text_color(value_color)
                .child(value),
        )
        // Source variable + offset.
        .child(
            div()
                .w(px(SOURCE_W))
                .flex_shrink_0()
                .overflow_hidden()
                .font_family(theme::MONO_FONT)
                .text_size(px(12.0))
                .text_color(theme::text_tertiary())
                .child(source),
        );

    if !tip.is_empty() {
        // gpui-component's tooltip lays its text out on a single line
        // with no width cap, so long help strings run off-screen —
        // hard-wrap to a readable column ourselves (newlines render as
        // line breaks).
        let tip = wrap(&tip, 64);
        row = row.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx));
    }
    // Ledger stripe for horizontal tracking.
    if virtual_i % 2 == 1 {
        row = row.bg(theme::workshop_glass());
    }
    row
}
