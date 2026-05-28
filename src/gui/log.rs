use super::{AppView, LogLine, format_log_time, theme};
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, ScrollHandle,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};
use gpui_component::scroll::ScrollableElement;
use gpui_component::tooltip::Tooltip;

pub fn render(
    lines: &[LogLine],
    local_tz: bool,
    scroll: &ScrollHandle,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
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
                        .font_family(theme::MONO_FONT)
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
                                        .child(format_log_time(line.timestamp_secs, local_tz)),
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
        .child(
            // Small × chip floating top-right of the panel. Sits
            // above the scroll content via `absolute()`, offset
            // enough on the right to clear the scrollbar gutter.
            // Hidden when there's nothing to clear so the panel
            // stays clean on first launch.
            div()
                .absolute()
                .top(px(6.0))
                .right(px(14.0))
                .when(!lines.is_empty(), |this| {
                    this.child(
                        div()
                            .id("log-clear")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(20.0))
                            .h(px(20.0))
                            .rounded(px(4.0))
                            .text_size(px(14.0))
                            .text_color(theme::text_tertiary())
                            .cursor_pointer()
                            .hover(|d| {
                                d.bg(theme::workshop_glass_strong())
                                    .text_color(theme::text_primary())
                            })
                            .child("\u{00D7}")
                            .tooltip(|window, cx| Tooltip::new("Clear log").build(window, cx))
                            .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _, cx| {
                                this.clear_log(cx);
                            })),
                    )
                }),
        )
        .vertical_scrollbar(scroll)
}
