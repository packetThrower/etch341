//! BIOS settings-diff window. Opened from the BIOS explorer's
//! "Compare with…" button: picks a second dump, diffs its Setup
//! settings against the loaded one, and shows the result in its own
//! wide window (same `open_window` + `Root` lifecycle as the chip-DB
//! browser). Read-only. GUI twin of the CLI `bios diff`.

use super::{AppView, theme};
use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, ScrollHandle,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, px,
};
use gpui_component::TitleBar;
use gpui_component::scroll::ScrollableElement;
use std::sync::Arc;

pub struct BiosDiffView {
    diffs: Arc<Vec<crate::uefi::SettingDiff>>,
    a_name: String,
    b_name: String,
    scroll: ScrollHandle,
    /// Weak handle to the main view, used only to clear
    /// `bios_diff_window` when this window closes.
    app: WeakEntity<AppView>,
}

impl BiosDiffView {
    pub fn new(
        app: WeakEntity<AppView>,
        diffs: Arc<Vec<crate::uefi::SettingDiff>>,
        a_name: String,
        b_name: String,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            diffs,
            a_name,
            b_name,
            scroll: ScrollHandle::new(),
            app,
        }
    }
}

impl Render for BiosDiffView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .px_5()
            .py_3()
            .text_size(px(13.0));

        let mut last_form = "\0";
        for d in self.diffs.iter() {
            let form = if d.form.is_empty() {
                "(uncategorised)"
            } else {
                d.form.as_str()
            };
            if form != last_form {
                body = body.child(group_header(form));
                last_form = form;
            }
            body = body.child(diff_row(d));
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bench_black())
            .text_color(theme::text_primary())
            .child(
                TitleBar::new()
                    .on_close_window({
                        let app = self.app.clone();
                        move |_, window, cx| {
                            let _ = app.update(cx, |this, cx| {
                                this.bios_diff_window = None;
                                cx.notify();
                            });
                            window.remove_window();
                        }
                    })
                    .child(
                        div()
                            .text_size(px(13.0))
                            .text_color(theme::text_secondary())
                            .child("BIOS Setting Diff"),
                    ),
            )
            .child(
                // A/B legend + count.
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(theme::workshop_glass_strong())
                    .text_size(px(12.0))
                    .child(
                        div()
                            .text_color(theme::diff_removed())
                            .child(format!("A  {}", self.a_name)),
                    )
                    .child(
                        div()
                            .text_color(theme::diff_added())
                            .child(format!("B  {}", self.b_name)),
                    )
                    .child(
                        div()
                            .pt_1()
                            .text_color(theme::text_tertiary())
                            .child(format!(
                                "{} setting{} differ",
                                self.diffs.len(),
                                if self.diffs.len() == 1 { "" } else { "s" }
                            )),
                    ),
            )
            .child(
                // Scrollable list; scrollbar is a SIBLING of the
                // scrolling child (not nested inside it).
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .id("bios-diff-list")
                            .size_full()
                            .track_scroll(&self.scroll)
                            .overflow_y_scroll()
                            .child(body),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
    }
}

/// A form section divider.
fn group_header(form: &str) -> impl IntoElement {
    div()
        .mt_2()
        .px_2()
        .py_1()
        .bg(theme::workshop_glass_strong())
        .rounded(px(4.0))
        .text_size(px(12.0))
        .text_color(theme::accent())
        .child(form.to_string())
}

/// One diff row: label · A value (red) → B value (green).
fn diff_row(d: &crate::uefi::SettingDiff) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_2()
        .py(px(3.0))
        .whitespace_nowrap()
        .child(
            div()
                .w(px(360.0))
                .flex_shrink_0()
                .overflow_hidden()
                .text_color(theme::text_primary())
                .child(d.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_color(theme::diff_removed())
                .child(d.a.clone().unwrap_or_else(|| "(absent)".into())),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme::text_tertiary())
                .child("→"),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_color(theme::diff_added())
                .child(d.b.clone().unwrap_or_else(|| "(absent)".into())),
        )
        .child(
            div()
                .w(px(210.0))
                .flex_shrink_0()
                .overflow_hidden()
                .font_family(theme::MONO_FONT)
                .text_size(px(11.0))
                .text_color(theme::text_tertiary())
                .child(format!("{}+0x{:04x}", d.varstore, d.offset)),
        )
}
