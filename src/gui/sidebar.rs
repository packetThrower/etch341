use super::{AppView, Bus, Pane, theme};
use gpui::{
    ClickEvent, Context, IntoElement, ParentElement, Styled as _, div, prelude::FluentBuilder as _,
    px,
};
use gpui_kit::component::badge::Badge;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::separator::Separator;
use gpui_kit::component::stepper::{Stepper, StepperItem};
use gpui_kit::component::{Disableable as _, Sizable as _, Size};

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
        .pb_3()
        .px_5()
        .gap_2()
        // SPI / I²C bus toggle — flips the whole workflow below it. Its
        // row matches the header strip's height so the two line up.
        .child(
            div()
                .h(px(super::header::HEIGHT))
                .flex()
                .items_center()
                .child(bus_toggle(bus, cx)),
        )
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

/// Two-segment SPI / I²C toggle (GPUI Kit `ButtonGroup`). The active
/// segment wears the accent; clicking the other flips the bus via
/// `AppView::set_bus`.
fn bus_toggle(bus: Bus, cx: &mut Context<AppView>) -> impl IntoElement {
    const BUSES: [(Bus, &str); 2] = [(Bus::Spi, "SPI"), (Bus::I2c, "I²C")];
    let weak = cx.entity().downgrade();
    let busy = super::hw_busy(cx);
    ButtonGroup::new("bus-toggle")
        .small()
        .w_full()
        .children(BUSES.iter().map(|&(target, label)| {
            // Locked while an op holds the CH341A: the workflow below
            // would swap to the other bus mid-operation.
            let b = Button::new(label).label(label).flex_1().disabled(busy);
            if target == bus { b.primary() } else { b }
        }))
        .on_click(move |clicked, _, cx| {
            let Some(&(target, _)) = clicked.first().and_then(|&i| BUSES.get(i)) else {
                return;
            };
            let _ = weak.update(cx, |this: &mut AppView, cx| this.set_bus(target, cx));
        })
}

/// A thin horizontal rule separating sidebar groups.
fn divider() -> impl IntoElement {
    Separator::horizontal().mt_2().mb_1()
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

/// Flat sidebar row (GPUI Kit `ListItem`) used for tool entries below
/// the rail. `dot` rides a small amber badge on the label (the "newer
/// release available" signal on Settings).
fn item(
    pane: Pane,
    label: &str,
    selected: Pane,
    dot: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let label = label.to_string();
    ListItem::new(gpui::SharedString::from(label.clone()))
        .selected(pane == selected)
        .px_3()
        .py_2()
        .rounded(px(6.0))
        .child(if dot {
            Badge::new()
                .dot()
                .color(theme::warning_amber())
                .child(label)
                .into_any_element()
        } else {
            label.into_any_element()
        })
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                if this.selected != pane {
                    this.disarm_all();
                }
                this.selected = pane;
                cx.notify();
            }),
        )
}
