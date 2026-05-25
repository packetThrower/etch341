use super::{theme, AppView, Pane};
use gpui::{
    div, px, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled,
};

pub fn render(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    match selected {
        Pane::Detect => detect_pane(cx).into_any_element(),
        Pane::Read => stub("Read", "Dump the chip contents to a file.").into_any_element(),
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
    div().text_color(theme::text_secondary()).child(text)
}

fn action_button(label: &'static str, cx: &mut Context<AppView>) -> impl IntoElement {
    div()
        .id(label)
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
        .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
            this.refresh_detect(cx);
        }))
}
