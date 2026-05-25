use super::{theme, LogLine};
use gpui::{
    div, px, InteractiveElement, IntoElement, ParentElement, ScrollHandle,
    StatefulInteractiveElement, Styled,
};
use gpui_component::scroll::ScrollableElement;

pub fn render(lines: &[LogLine], scroll: &ScrollHandle) -> impl IntoElement {
    // Baudrun pattern: outer `.relative()` is the positioning context;
    // the scrollable element and the `vertical_scrollbar` are siblings
    // inside it. Earlier we had the scrollbar as a child of the
    // scrollable, which caused the scrollbar itself to scroll along
    // with the content — looked "backwards" because the thumb stayed
    // near the visual top of the content rather than the viewport.
    div()
        .relative()
        .h(px(240.0))
        .border_t_1()
        .border_color(theme::workshop_glass_strong())
        .bg(theme::bench_black())
        .child(
            div()
                .id("activity-log")
                .size_full()
                .track_scroll(scroll)
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_5()
                        .py_3()
                        .font_family("Menlo")
                        .text_size(px(12.0))
                        .text_color(theme::text_secondary())
                        .children(lines.iter().map(|line| {
                            div()
                                .flex()
                                .flex_row()
                                .gap_3()
                                .child(
                                    div()
                                        .text_color(theme::text_tertiary())
                                        .child(line.timestamp.clone()),
                                )
                                .child(div().child(line.text.clone()))
                        })),
                ),
        )
        .vertical_scrollbar(scroll)
}
