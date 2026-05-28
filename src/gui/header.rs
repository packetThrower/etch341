use super::{Connection, SharedProgress, theme};
use gpui::{IntoElement, ParentElement, Styled, div, px};
use std::sync::atomic::Ordering;

pub fn render(conn: &Connection, progress: &SharedProgress) -> impl IntoElement {
    let (dot, label) = match conn {
        Connection::Disconnected => (theme::caution_red(), "no CH341A".to_string()),
        Connection::NoChip => (theme::warning_amber(), "CH341A · no chip".to_string()),
        Connection::Ready { chip_name, size_kb } => (
            theme::success_green(),
            format!("CH341A · {chip_name} · {} KB", size_kb),
        ),
    };

    // Right-hand activity tag: "idle" when nothing's running, otherwise
    // "<label> 27% (140 KB / 512 KB)" updated by the polling task.
    let active = progress.active.load(Ordering::Relaxed);
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
        .py_3()
        .border_b_1()
        .border_color(theme::workshop_glass_strong())
        .child(div().w(px(8.0)).h(px(8.0)).rounded_full().bg(dot))
        .child(div().text_color(theme::text_primary()).child(label))
        .child(div().flex_1())
        .child(activity_div)
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
