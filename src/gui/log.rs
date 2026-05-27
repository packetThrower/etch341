use super::{LogLine, theme};
use gpui::{
    InteractiveElement, IntoElement, ParentElement, ScrollHandle, StatefulInteractiveElement,
    Styled, div, px,
};
use gpui_component::scroll::ScrollableElement;

pub fn render(lines: &[LogLine], scroll: &ScrollHandle) -> impl IntoElement {
    // The outer `.relative()` is the positioning context; the
    // scrollable element and the `vertical_scrollbar` must be
    // SIBLINGS inside it — not parent and child. If the scrollbar
    // is a child of the scrolling element, it scrolls along with
    // the content and the thumb stays near the visual top of the
    // content rather than the viewport, which looks "backwards".
    // `size_full()` (not just `h(px(180.0))`) — the resizable_panel
    // parent already drives the height, but without an explicit
    // width the outer div shrinks to fit its longest log line and
    // the black background ends short of the window's right edge.
    // `size_full` fills both axes inside the panel.
    div()
        .relative()
        .size_full()
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
                                .child(
                                    // Wrap long log lines (file paths,
                                    // error messages) instead of letting
                                    // them run off the right edge.
                                    div().flex_1().whitespace_normal().child(line.text.clone()),
                                )
                        })),
                ),
        )
        .vertical_scrollbar(scroll)
}
