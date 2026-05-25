use super::{theme, AppView, Pane};
use gpui::{
    div, prelude::FluentBuilder, px, ClickEvent, Context, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled,
};

pub fn render(
    selected: Pane,
    erase_armed: bool,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    match selected {
        Pane::Detect => detect_pane(cx).into_any_element(),
        Pane::Read => read_pane(cx).into_any_element(),
        Pane::Erase => erase_pane(erase_armed, cx).into_any_element(),
        Pane::Write => stub("Write", "Program the chip from a file.").into_any_element(),
        Pane::Verify => {
            stub("Verify", "Compare a file against the chip without writing.").into_any_element()
        }
        Pane::Blank => blank_pane(cx).into_any_element(),
        Pane::Settings => stub(
            "Settings",
            "SPI clock speed, chip DB override, preferences.",
        )
        .into_any_element(),
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

fn stub(title: &'static str, body_text: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .px_5()
        .py_5()
        .child(heading(title))
        .child(body(body_text))
        .child(
            div()
                .text_color(theme::text_tertiary())
                .text_size(px(11.0))
                .child("(not wired yet — coming in next iteration)"),
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
        .on_click(cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
            on_click(this, cx);
        }))
}
