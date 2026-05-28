use super::{AppView, LogLine, format_log_time, theme};
use gpui::{
    ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render,
    ScrollHandle, Stateful, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    div, prelude::FluentBuilder, px,
};
use gpui_component::TitleBar;
use gpui_component::scroll::ScrollableElement;
use gpui_component::tooltip::Tooltip;

/// Inline activity-log panel rendered at the bottom of the main
/// window's pane / log split. The top-right chips pop the log out
/// into its own window and clear it.
pub fn render(
    lines: &[LogLine],
    local_tz: bool,
    scroll: &ScrollHandle,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let buttons = div()
        .flex()
        .flex_row()
        .gap_1()
        .child(
            chip("log-popout", "\u{29C9}", "Pop out into a window").on_click(cx.listener(
                |this: &mut AppView, _: &ClickEvent, _, cx| {
                    this.pop_out_log(cx);
                },
            )),
        )
        .when(!lines.is_empty(), |row| {
            row.child(
                chip("log-clear", "\u{00D7}", "Clear log").on_click(cx.listener(
                    |this: &mut AppView, _: &ClickEvent, _, cx| {
                        this.clear_log(cx);
                    },
                )),
            )
        });
    surface(lines, local_tz, scroll, buttons)
}

/// Root view for the detached log window. Holds a weak handle back
/// to the main `AppView` and re-renders whenever it notifies (every
/// push_log / clear / timezone toggle calls `cx.notify()`), so the
/// pop-out stays live without owning a second copy of the log
/// buffer. The buffer of record stays on `AppView`.
pub struct LogWindow {
    app: WeakEntity<AppView>,
    scroll: ScrollHandle,
    _observe: Subscription,
}

impl LogWindow {
    pub fn new(app: Entity<AppView>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&app, |_this, _app, cx| cx.notify());
        Self {
            app: app.downgrade(),
            scroll: ScrollHandle::new(),
            _observe: observe,
        }
    }
}

impl Render for LogWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Read the live buffer straight off AppView each frame. If
        // the main window is gone (app shutting down) fall back to an
        // empty log rather than panicking on a dead weak handle.
        let (lines, local) = self
            .app
            .upgrade()
            .map(|app| {
                let a = app.read(cx);
                (a.log_lines.clone(), a.prefs.timestamp_local)
            })
            .unwrap_or_default();

        let app = self.app.clone();
        let buttons = div()
            .flex()
            .flex_row()
            .gap_1()
            .when(!lines.is_empty(), move |row| {
                row.child(chip("log-clear-popout", "\u{00D7}", "Clear log").on_click(
                    move |_: &ClickEvent, _, cx| {
                        let _ = app.update(cx, |this, cx| this.clear_log(cx));
                    },
                ))
            });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bench_black())
            .text_color(theme::text_primary())
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme::text_secondary())
                        .child("Activity Log"),
                ),
            )
            .child(div().flex_1().min_h(px(0.0)).child(surface(
                &lines,
                local,
                &self.scroll,
                buttons,
            )))
    }
}

/// Shared bordered, scrollable log surface used by both the inline
/// panel and the pop-out window. `buttons` is the top-right overlay
/// (pop-out / clear chips) which differs per call site, so the
/// caller passes it in already built.
fn surface(
    lines: &[LogLine],
    local_tz: bool,
    scroll: &ScrollHandle,
    buttons: impl IntoElement,
) -> impl IntoElement {
    // The outer `.relative()` is the positioning context; the
    // scrollable element and the `vertical_scrollbar` must be
    // SIBLINGS inside it — not parent and child. If the scrollbar
    // is a child of the scrolling element, it scrolls along with
    // the content and the thumb stays near the visual top of the
    // content rather than the viewport, which looks "backwards".
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
                        .children(lines.iter().map(|line| log_row(line, local_tz))),
                ),
        )
        .child(
            // Chips float top-right, offset enough on the right to
            // clear the scrollbar gutter.
            div().absolute().top(px(6.0)).right(px(14.0)).child(buttons),
        )
        .vertical_scrollbar(scroll)
}

fn log_row(line: &LogLine, local_tz: bool) -> impl IntoElement {
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
            // Wrap long log lines (file paths, error messages) instead
            // of letting them run off the right edge.
            div().flex_1().whitespace_normal().child(line.text.clone()),
        )
}

/// Small 20×20 hover-tinted glyph chip with a tooltip. The caller
/// attaches `.on_click(...)` — left off here because the inline
/// panel dispatches on `AppView` directly while the pop-out window
/// dispatches through a weak handle.
fn chip(id: &'static str, glyph: &'static str, tip: &'static str) -> Stateful<gpui::Div> {
    div()
        .id(id)
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
        .child(glyph)
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
}
