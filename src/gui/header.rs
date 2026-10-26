use super::{AppView, Connection, SharedProgress, theme};
use gpui::{
    ClickEvent, Context, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Ops that stop cleanly at a chunk / page boundary. SPI erase is one
/// chip-erase command the chip finishes on its own, and the I²C
/// write/erase go out in a single call, so those get no Cancel.
const CANCELLABLE: &[&str] = &[
    "read",
    "verify",
    "blank",
    "write",
    "i2c-read",
    "i2c-vfy",
    "i2c-blank",
];

/// Height of the header strip. The sidebar's bus-toggle row uses the
/// same height so the toggle lines up with the connection status.
pub const HEIGHT: f32 = 52.0;

pub fn render(
    conn: &Connection,
    progress: &Arc<SharedProgress>,
    cx: &mut Context<AppView>,
) -> impl IntoElement {
    let (dot, label) = match conn {
        Connection::Disconnected => (theme::caution_red(), "no CH341A".to_string()),
        Connection::Present => (theme::success_green(), "CH341A".to_string()),
        Connection::NoChip => (theme::warning_amber(), "CH341A · no chip".to_string()),
        Connection::Ready { chip_name, size } => (
            theme::success_green(),
            format!("CH341A · {chip_name} · {size}"),
        ),
    };

    // Right-hand activity tag: "idle" when nothing's running, otherwise
    // "<label> 27% (140 KB / 512 KB)" updated by the polling task.
    let active = progress.active.load(Ordering::Relaxed);
    let cancellable = CANCELLABLE.contains(&progress.label.lock().unwrap().as_str());
    let activity = if active {
        let current = progress.current.load(Ordering::Relaxed);
        let total = progress.total.load(Ordering::Relaxed);
        let label = progress.label.lock().unwrap().clone();
        if let Some(pct) = (current * 100).checked_div(total) {
            format!(
                "{label} {pct}% ({} / {})",
                fmt_bytes(current),
                fmt_bytes(total)
            )
        } else {
            format!("{label}…")
        }
    } else {
        "idle".to_string()
    };
    // While an op is running, render the activity tag as an
    // accent-blue pill so it stands out instead of fading into the
    // header's text-tertiary "idle" treatment. The user reported
    // the previous gray-on-dark progress was barely noticeable
    // — easy to miss whether a Read/Write was running at all.
    let activity_div = if active {
        div()
            .px_2()
            .py_0p5()
            .rounded(px(4.0))
            .bg(theme::accent_tint())
            .text_color(theme::accent())
            .child(activity)
    } else {
        div().text_color(theme::text_tertiary()).child(activity)
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_5()
        .h(px(HEIGHT))
        .border_b_1()
        .border_color(theme::workshop_glass_strong())
        .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(dot))
        .child(div().text_color(theme::text_primary()).child(label))
        .child(div().flex_1())
        .child(activity_div)
        .when(active && cancellable, |row| {
            row.child(cancel_button(progress, cx))
        })
}

/// Ghost "Cancel" beside the progress pill. The op stops at its next
/// chunk / erase-block / page boundary; a write is left partly done
/// (erased, or partly programmed), hence the tooltip.
fn cancel_button(progress: &Arc<SharedProgress>, cx: &mut Context<AppView>) -> impl IntoElement {
    let progress = progress.clone();
    let tip = if *progress.label.lock().unwrap() == "write" {
        "Stops after the current erase block or page — the chip is left partly erased / written"
    } else {
        "Stops after the current chunk"
    };
    Button::new("cancel-op")
        .label("Cancel")
        .ghost()
        .small()
        .tooltip(tip)
        .on_click(
            cx.listener(move |this: &mut AppView, _: &ClickEvent, _, cx| {
                if !progress.cancel.swap(true, Ordering::Relaxed) {
                    this.push_log("Cancelling…".into());
                    cx.notify();
                }
            }),
        )
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.1} KB", n as f64 / (1u64 << 10) as f64)
    } else {
        format!("{n} B")
    }
}
