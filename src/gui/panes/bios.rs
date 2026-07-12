//! BIOS Setup explorer pane: pick a flash image, list its Setup
//! options as label → current value → choices, filterable by label.
//! The read-only GUI twin of `etch341 bios settings`.

// The parent module is this submodule's prelude (see panes.rs).
use super::*;

/// Fixed row height so `uniform_list` can virtualise the (often
/// thousands of) settings without measuring each row.
const ROW_H: f32 = 30.0;

pub(super) fn bios_pane(
    path: Option<&Path>,
    settings: Option<Arc<Vec<crate::uefi::Setting>>>,
    scroll: UniformListScrollHandle,
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
             against the NVRAM store. Read-only. A ✷ marks options the firmware \
             may hide or lock at runtime.",
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
        );

    let Some(settings) = settings else {
        // Nothing loaded yet — the file row above is the whole pane.
        return col;
    };

    // Filter by label (case-insensitive substring), mirroring the CLI's
    // `--find`. Precompute the surviving indices so `uniform_list` can
    // map virtual rows onto the shared `Arc` without cloning the Vec.
    let needle = search_term.to_lowercase();
    let visible: Arc<Vec<usize>> = Arc::new(
        settings
            .iter()
            .enumerate()
            .filter(|(_, s)| needle.is_empty() || s.name.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect(),
    );

    col = col
        // Row wrapper so the Input's flex_1 grows horizontally; placing
        // flex_1 directly in this column would stretch it vertically and
        // shove the list down the pane.
        .child(
            div()
                .flex()
                .flex_row()
                .child(div().flex_1().child(Input::new(search_state))),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_tertiary())
                .child(format!(
                    "{} of {} settings{}",
                    visible.len(),
                    settings.len(),
                    if needle.is_empty() {
                        String::new()
                    } else {
                        format!(" matching “{search_term}”")
                    },
                )),
        )
        .child(settings_list(settings, visible, scroll));

    col
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

/// The virtualised list of setting rows.
fn settings_list(
    settings: Arc<Vec<crate::uefi::Setting>>,
    visible: Arc<Vec<usize>>,
    scroll: UniformListScrollHandle,
) -> impl IntoElement {
    let count = visible.len();
    uniform_list("bios-settings-list", count, move |range, _, _| {
        range
            .map(|virtual_i| setting_row(&settings[visible[virtual_i]], virtual_i))
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
    .track_scroll(&scroll)
}

/// One setting row: label · current value · source (variable+offset),
/// with a hover tooltip carrying the help text and the full choice
/// list. Single line at a fixed height for the virtualised list.
fn setting_row(s: &crate::uefi::Setting, virtual_i: usize) -> impl IntoElement + use<> {
    let value = match (&s.value_label, s.value) {
        (Some(label), Some(v)) => format!("{label} (0x{v:x})"),
        (None, Some(v)) => format!("0x{v:x}"),
        _ => "—".to_string(),
    };
    let value_color = if s.value.is_some() {
        theme::success_green()
    } else {
        theme::text_tertiary()
    };

    // Tooltip: help line (if any) plus the choices behind the byte.
    let mut tip = s.help.clone();
    if !s.options.is_empty() {
        let choices: Vec<&str> = s
            .options
            .iter()
            .map(|(_, l)| l.as_str())
            .filter(|l| !l.is_empty())
            .collect();
        if !choices.is_empty() {
            if !tip.is_empty() {
                tip.push_str("\n\n");
            }
            tip.push_str(&format!("Choices: {}", choices.join(" / ")));
        }
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
        .px_2()
        .whitespace_nowrap()
        // Label — flexes to fill, truncates when long.
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_color(theme::text_primary())
                .child(s.name.clone()),
        )
        // Conditional marker — fixed slot, always present.
        .child(
            div()
                .w(px(16.0))
                .flex_shrink_0()
                .text_color(theme::warning_amber())
                .child(if s.conditional { "✷" } else { "" }),
        )
        // Current value.
        .child(
            div()
                .w(px(200.0))
                .flex_shrink_0()
                .overflow_hidden()
                .text_color(value_color)
                .child(value),
        )
        // Source variable + offset.
        .child(
            div()
                .w(px(190.0))
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
