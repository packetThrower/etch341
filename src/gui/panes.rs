use super::{theme, AppView, Pane};
use gpui::{
    div, px, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled,
};

pub fn render(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    match selected {
        Pane::Detect => detect_pane(cx).into_any_element(),
        Pane::Read => read_pane(cx).into_any_element(),
        Pane::Erase => stub("Erase", "Erase the entire chip or a specific range. Destructive.")
            .into_any_element(),
        Pane::Write => stub("Write", "Program the chip from a file.").into_any_element(),
        Pane::Verify => {
            stub("Verify", "Compare a file against the chip without writing.").into_any_element()
        }
        Pane::Blank => {
            stub("Blank check", "Confirm the chip reads back as all 0xFF.").into_any_element()
        }
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
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(110.0))
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
