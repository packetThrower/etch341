use super::{AppView, Bus, Pane, theme};
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled as _, div, prelude::FluentBuilder as _, px,
};
use gpui_component::stepper::{Stepper, StepperItem};
use gpui_component::{Sizable as _, Size};

/// SPI workflow steps, in order. The slice position is the Stepper
/// step index. Blank check / Status / Security / Hex are tools, not
/// steps, so they sit below the rail.
const WORKFLOW: &[(Pane, &str)] = &[
    (Pane::Detect, "Detect"),
    (Pane::Read, "Read"),
    (Pane::Erase, "Erase"),
    (Pane::Write, "Write"),
    (Pane::Verify, "Verify"),
];

/// I²C workflow steps — mirrors the SPI rail one-for-one (Scan stands
/// in for Detect). Blank check is a tool below the rail, same as SPI.
const I2C_WORKFLOW: &[(Pane, &str)] = &[
    (Pane::I2cScan, "Scan"),
    (Pane::I2cRead, "Read"),
    (Pane::I2cErase, "Erase"),
    (Pane::I2cWrite, "Write"),
    (Pane::I2cVerify, "Verify"),
];

pub fn render(selected: Pane, bus: Bus, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .w(px(180.0))
        .h_full()
        .bg(theme::workshop_glass())
        // Just enough to clear the macOS traffic lights that float over
        // the sidebar's top-left — the full-width toggle can't sit any
        // higher without colliding with them.
        .pt(px(30.0))
        .pb_3()
        .px_5()
        .gap_2()
        // SPI / I²C bus toggle — flips the whole workflow below it.
        .child(bus_toggle(bus, cx))
        .child(match bus {
            Bus::Spi => spi_workflow(selected, cx).into_any_element(),
            Bus::I2c => i2c_workflow(selected, cx).into_any_element(),
        })
        .child(div().flex_1())
        .child(item(
            Pane::Settings,
            "⚙ Settings",
            selected,
            super::updater::available(cx).is_some(),
            cx,
        ))
}

/// Two-segment SPI / I²C toggle. The active segment wears the accent;
/// clicking the other flips the bus via `AppView::set_bus`.
fn bus_toggle(bus: Bus, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap_1()
        .p(px(2.0))
        // Breathing room below the toggle, above the workflow (on top
        // of the sidebar's `gap_2`).
        .mb_3()
        .rounded(px(8.0))
        .bg(theme::workshop_glass_strong())
        .child(bus_seg("SPI", Bus::Spi, bus, cx))
        .child(bus_seg("I²C", Bus::I2c, bus, cx))
}

fn bus_seg(
    label: &'static str,
    target: Bus,
    current: Bus,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let active = target == current;
    div()
        .id(label)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .py_1()
        .rounded(px(6.0))
        .text_size(px(12.0))
        .cursor_pointer()
        .text_color(if active {
            theme::accent_foreground()
        } else {
            theme::text_secondary()
        })
        .when(active, |d| d.bg(theme::accent()))
        .when(!active, |d| {
            d.hover(|h| {
                h.bg(theme::workshop_glass())
                    .text_color(theme::text_primary())
            })
        })
        .child(label)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                this.set_bus(target, cx);
            }),
        )
}

/// A thin horizontal rule separating sidebar groups.
fn divider() -> impl IntoElement {
    div()
        .h(px(1.0))
        .mt_2()
        .mb_1()
        .bg(theme::workshop_glass_strong())
}

/// The vertical Stepper rail for a workflow. Highlights the active
/// step; when the selected pane is a tool (not in `workflow`) the rail
/// collapses to step 0 and dims so it reads as a reference rather than
/// a progress indicator. Shared by both buses so I²C looks like SPI.
fn stepper_rail(
    workflow: &'static [(Pane, &'static str)],
    id: &'static str,
    selected: Pane,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let on_workflow = workflow.iter().any(|(p, _)| *p == selected);
    let step = workflow
        .iter()
        .position(|(p, _)| *p == selected)
        .unwrap_or(0);

    let weak = cx.entity().downgrade();
    let stepper = Stepper::new(id)
        .vertical()
        .with_size(Size::Large)
        .selected_index(step)
        .items(workflow.iter().map(|(_, label)| {
            StepperItem::new().pb(px(30.0)).child(
                div()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .child(label.to_string()),
            )
        }))
        .on_click(move |&step, _window, cx_app| {
            let Some((pane, _)) = workflow.get(step) else {
                return;
            };
            let pane = *pane;
            let _ = weak.update(cx_app, |this: &mut AppView, ctx| {
                if this.selected != pane {
                    this.disarm_all();
                }
                this.selected = pane;
                ctx.notify();
            });
        });

    div().when(!on_workflow, |d| d.opacity(0.4)).child(stepper)
}

/// SPI: the workflow rail, then the SPI-only diagnostics (Blank /
/// Status / Security) and the shared Hex viewer.
fn spi_workflow(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(stepper_rail(WORKFLOW, "etch-spi-rail", selected, cx))
        .child(divider())
        .child(item(Pane::Blank, "Blank check", selected, false, cx))
        .child(item(Pane::Status, "Status regs", selected, false, cx))
        .child(item(Pane::Otp, "Security regs", selected, false, cx))
        .child(item(Pane::Hex, "Hex viewer", selected, false, cx))
        .child(item(Pane::Bios, "BIOS explorer", selected, false, cx))
}

/// I²C: the same rail shape, then Blank check and the shared Hex
/// viewer as tools. No status/security — EEPROMs have neither.
fn i2c_workflow(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(stepper_rail(I2C_WORKFLOW, "etch-i2c-rail", selected, cx))
        .child(divider())
        .child(item(Pane::I2cBlank, "Blank check", selected, false, cx))
        .child(item(Pane::Hex, "Hex viewer", selected, false, cx))
}

/// Flat sidebar row used for tool entries below the rail. `dot` rides
/// a small amber indicator on the right edge (the "newer release
/// available" signal on Settings).
fn item(
    pane: Pane,
    label: &str,
    selected: Pane,
    dot: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let active = pane == selected;
    let mut row = div()
        .id(gpui::SharedString::from(label.to_string()))
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
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
        .when(dot, |row| {
            row.child(
                div()
                    .w(px(8.0))
                    .h(px(8.0))
                    .rounded_full()
                    .bg(theme::warning_amber()),
            )
        })
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                if this.selected != pane {
                    this.disarm_all();
                }
                this.selected = pane;
                cx.notify();
            }),
        );
    if active {
        row = row.bg(theme::accent_tint());
    }
    row
}
