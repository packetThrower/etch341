use super::{AppView, Pane, theme};
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

pub fn render(selected: Pane, cx: &mut Context<AppView>) -> impl IntoElement {
    let items: [(Pane, &str); 7] = [
        (Pane::Detect, "Detect"),
        (Pane::Read, "Read"),
        (Pane::Erase, "Erase"),
        (Pane::Write, "Write"),
        (Pane::Verify, "Verify"),
        (Pane::Blank, "Blank check"),
        (Pane::Hex, "Hex viewer"),
    ];

    // Build the column with a sequential for-loop rather than
    // `iter().map(...)` so each `item()` call's borrow on `cx`
    // releases before the next one starts. The map+closure form
    // makes the closure FnMut and the captured `&mut cx` can't
    // escape across iterations.
    let mut root = div()
        .flex()
        .flex_col()
        .w(px(220.0))
        .h_full()
        .bg(theme::workshop_glass())
        .pt(px(48.0))
        .pb_3()
        .px_2()
        .gap_1();
    for (p, l) in items {
        root = root.child(item(p, l, selected, cx));
    }
    root.child(div().flex_1())
        .child(item(Pane::Settings, "⚙ Settings", selected, cx))
}

fn item(
    pane: Pane,
    label: &'static str,
    selected: Pane,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let active = pane == selected;
    let mut row = div()
        .id(label)
        .flex()
        .items_center()
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
        .hover(|d| d.bg(theme::workshop_glass_strong()))
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                // Navigating away from any destructive pane re-disarms
                // its trigger. Without this, an armed action could fire
                // on a stale click if the user returns to the pane and
                // accidentally clicks once more.
                if this.selected != pane {
                    this.erase_armed = false;
                    this.write_armed = false;
                }
                this.selected = pane;
                cx.notify();
            }),
        );
    if active {
        row = row.bg(theme::accent_blue_tint());
    }
    row
}
