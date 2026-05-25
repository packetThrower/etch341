use super::{theme, Connection};
use gpui::{div, px, IntoElement, ParentElement, Styled};

pub fn render(conn: &Connection) -> impl IntoElement {
    let (dot, label) = match conn {
        Connection::Disconnected => (theme::caution_red(), "no CH341A".to_string()),
        Connection::NoChip => (theme::warning_amber(), "CH341A · no chip".to_string()),
        Connection::Ready { chip_name, size_kb } => (
            theme::success_green(),
            format!("CH341A · {chip_name} · {} KB", size_kb),
        ),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_5()
        .py_3()
        .border_b_1()
        .border_color(theme::workshop_glass_strong())
        .child(
            div()
                .w(px(8.0))
                .h(px(8.0))
                .rounded_full()
                .bg(dot),
        )
        .child(div().text_color(theme::text_primary()).child(label))
        .child(div().flex_1())
        .child(
            div()
                .text_color(theme::text_tertiary())
                .child("idle"),
        )
}
