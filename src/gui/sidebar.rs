use super::{AppView, Pane, theme};
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::{Sizable as _, Size};

/// Workflow panes in the order users typically perform them. The
/// position in this slice is the step index passed to / received
/// from `Stepper`. Hex viewer / Blank check / Settings are
/// inspection or diagnostic tools — not steps in the linear flow
/// — so they sit below the stepper as flat items instead of
/// adding "extra" trailing rungs that confuse the rail's reading
/// as a workflow.
const WORKFLOW: &[(Pane, &str)] = &[
    (Pane::Detect, "Detect"),
    (Pane::Read, "Read"),
    (Pane::Erase, "Erase"),
    (Pane::Write, "Write"),
    (Pane::Verify, "Verify"),
];

pub fn render(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    // Map the currently-selected Pane to a step index. The Stepper
    // widget can't represent "nothing selected" — any
    // `checked_step >= 0` highlights at least step 0 — so when the
    // user is on a non-workflow pane (Blank check / Hex viewer /
    // Settings) we collapse to step 0 *and* dim the whole stepper
    // (via the `off_workflow` opacity below) to signal "this is a
    // reference for the canonical workflow, you're not in it
    // right now". Previously we used `WORKFLOW.len()` here, which
    // marked every workflow item as "passed" (white-filled) —
    // misleading: it implied Verify had just completed when the
    // user had simply navigated to Hex viewer.
    let on_workflow = WORKFLOW.iter().any(|(p, _)| *p == selected);
    let step = WORKFLOW
        .iter()
        .position(|(p, _)| *p == selected)
        .unwrap_or(0);

    // The Stepper's `on_click` callback signature is `Fn(&usize,
    // &mut Window, &mut App)` — no `Context<AppView>`. Use a
    // downgraded weak entity to call back into AppView through
    // `weak.update(cx_app, |this, ctx| ...)`. Pattern matches what
    // we already do for `v_resizable.on_resize` in `gui::mod`.
    let weak = cx.entity().downgrade();
    let stepper = Stepper::new("etch-workflow")
        .vertical()
        // `Size::Large` bumps the step glyph to 32px (default is
        // 24px), which both reads better at our sidebar width and
        // — more importantly — gives the absolute-positioned
        // separator something to span. With default size and no
        // per-item padding the items render touching, which hides
        // the connecting rail and squishes the labels.
        .with_size(Size::Large)
        .selected_index(step)
        .items(WORKFLOW.iter().map(|(_, label)| {
            // Per-item bottom padding gives the connecting rail
            // breathing room — `Large` glyphs are 32px, so the
            // separator only paints when the item is taller than
            // 32px.
            //
            // The label is wrapped in a 32px-tall flex container
            // that vertically centers the text. The Stepper's
            // built-in trigger lays out the glyph + label row
            // with `items_start` (top-aligned across the gap-2
            // gap), which leaves the shorter label visually
            // floating near the top of the 32px circle. Giving
            // the label its own equal-height box with
            // `items_center` lines its baseline up with the
            // middle of the glyph without us forking
            // gpui-component's trigger.
            StepperItem::new().pb(px(30.0)).child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .child(label.to_string()),
            )
        }))
        .on_click(move |&step, _window, cx_app| {
            let Some((pane, _)) = WORKFLOW.get(step) else {
                return;
            };
            let pane = *pane;
            let _ = weak.update(cx_app, |this: &mut AppView, ctx| {
                // Navigating away from any destructive pane re-disarms
                // its trigger. Without this, an armed action could
                // fire on a stale click if the user returns to the
                // pane and accidentally clicks once more.
                if this.selected != pane {
                    this.erase_armed = false;
                    this.write_armed = false;
                }
                this.selected = pane;
                ctx.notify();
            });
        });

    div()
        .flex()
        .flex_col()
        // 180px down from the original 220px — the stepper's
        // glyph + short labels ("Detect", "Read", "Erase",
        // "Write", "Verify") leave ~80px of slack at 220px. The
        // longest label below the divider is "Blank check" / "Hex
        // viewer" / "⚙ Settings" at ~92px including the glyph,
        // which fits comfortably at 180px while reclaiming 40px
        // of horizontal real estate for the main pane.
        .w(px(180.0))
        .h_full()
        .bg(theme::workshop_glass())
        .pt(px(48.0))
        .pb_3()
        // px_5 (20px) matches the standard padding used by the
        // operation panes on the right — same visual gutter on
        // both sides of the splitter. The earlier `px_3` (12px)
        // pushed the stepper glyphs almost against the window
        // edge, which competed for the eye with the macOS
        // traffic-light overlay above them.
        .px_5()
        .gap_1()
        // When the active pane isn't part of the linear workflow
        // (Hex viewer / Blank check / Settings), dim the stepper
        // so it reads as a reference rather than a progress
        // indicator. Click-through still works at full opacity —
        // GPUI's `opacity` is paint-only.
        .child(
            div()
                .when(!on_workflow, |d| d.opacity(0.4))
                .child(stepper),
        )
        // Diagnostic / inspection tools live below the stepper —
        // a thin divider sets them apart from the workflow rail.
        // Blank check belongs here (not in the stepper) because
        // it's the post-erase sanity check, not part of the
        // canonical read → erase → write → verify sequence; users
        // reach for it after Erase to confirm the chip is actually
        // 0xFF, or on a fresh new chip before writing.
        .child(
            div()
                .h(px(1.0))
                .mt_2()
                .mb_1()
                .bg(theme::workshop_glass_strong()),
        )
        .child(item(Pane::Blank, "Blank check", selected, cx))
        .child(item(Pane::Status, "Status regs", selected, cx))
        .child(item(Pane::Hex, "Hex viewer", selected, cx))
        .child(div().flex_1())
        .child(item(Pane::Settings, "⚙ Settings", selected, cx))
}

/// Flat sidebar row used for the non-workflow entries (Hex /
/// Settings). Mirrors the original sidebar's `item()` body — the
/// stepper handles the workflow rows with its own rendering.
fn item(
    pane: Pane,
    label: &'static str,
    selected: Pane,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let active = pane == selected;
    let mut row = div()
        .id(label)
        .flex()
        .items_center()
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .cursor_pointer()
        .text_color(if active {
            theme::text_primary()
        } else {
            theme::text_secondary()
        })
        .child(label.to_string())
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                if this.selected != pane {
                    this.erase_armed = false;
                    this.write_armed = false;
                }
                this.selected = pane;
                cx.notify();
            }),
        );
    if active {
        row = row.bg(theme::accent_blue_tint());
    }
    row
}
