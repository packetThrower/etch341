//! GPUI frontend. Compiled only with the `gui` feature.

use gpui::{
    div, px, App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, ScrollHandle,
    Styled, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{Root, TitleBar};

use crate::ch341::Ch341;
use crate::ops::{self, Diagnosis};

mod header;
mod log;
mod panes;
mod sidebar;
mod theme;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(|cx: &mut App| {
        gpui_component::init(cx);

        let bounds = Bounds::centered(None, gpui::size(px(1000.0), px(700.0)), cx);
        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("etch341".into()),
                    traffic_light_position: Some(gpui::point(px(16.0), px(16.0))),
                    ..TitleBar::title_bar_options()
                }),
                app_id: Some("etch341".into()),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| AppView::new());
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            eprintln!("etch341: failed to open window: {err}");
        }
    });
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Detect,
    Read,
    Erase,
    Write,
    Verify,
    Blank,
    Settings,
}

#[derive(Clone, Debug)]
pub enum Connection {
    Disconnected,
    NoChip,
    Ready { chip_name: String, size_kb: u32 },
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub timestamp: String,
    pub text: String,
}

pub struct AppView {
    pub selected: Pane,
    pub connection: Connection,
    pub log_lines: Vec<LogLine>,
    /// Persists scroll position across re-renders; required by
    /// `track_scroll(...)` to keep the log from jumping back to the
    /// top whenever a new line is appended.
    pub log_scroll: ScrollHandle,
}

impl AppView {
    pub fn new() -> Self {
        Self {
            selected: Pane::Detect,
            connection: Connection::Disconnected,
            log_lines: vec![LogLine {
                timestamp: now_hms(),
                text: "etch341 ready — plug in a CH341A and click Refresh".into(),
            }],
            log_scroll: ScrollHandle::new(),
        }
    }

    /// Run `ops::run_detect` synchronously on the UI thread and fold the
    /// result into the session header + activity log. USB enumeration +
    /// a 4-byte SPI transfer is ~50 ms in practice; acceptable while we
    /// stay single-threaded. Long ops (read/erase/write) will move to a
    /// background task once they're wired.
    pub fn refresh_detect(&mut self, cx: &mut Context<Self>) {
        self.push_log(format!("→ detect"));
        let outcome = Ch341::open(false).and_then(|mut ch| ops::run_detect(&mut ch));
        match outcome {
            Ok(result) => {
                self.push_log(format!("JEDEC 0x{}", result.jedec_string()));
                match result.diagnosis {
                    Diagnosis::Known(c) => {
                        self.push_log(format!("Detected {} ({} KB)", c.name, c.size_kb));
                        self.connection = Connection::Ready {
                            chip_name: c.name,
                            size_kb: c.size_kb,
                        };
                    }
                    Diagnosis::UnknownChip => {
                        self.push_log(format!(
                            "Unknown JEDEC 0x{} — add an entry to chips.toml",
                            result.jedec_string()
                        ));
                        self.connection = Connection::NoChip;
                    }
                    Diagnosis::MisoStuckLow => {
                        self.push_log(
                            "MISO stuck low — target board contention (lift chip or pin 8)".into(),
                        );
                        self.connection = Connection::NoChip;
                    }
                    Diagnosis::MisoFloatsHigh => {
                        self.push_log(
                            "MISO floats high — no chip detected (check clip, VCC, pin 1)".into(),
                        );
                        self.connection = Connection::NoChip;
                    }
                }
            }
            Err(err) => {
                self.push_log(format!("error: {err}"));
                self.connection = Connection::Disconnected;
            }
        }
        cx.notify();
    }

    fn push_log(&mut self, text: String) {
        self.log_lines.push(LogLine {
            timestamp: now_hms(),
            text,
        });
        // Snap the scroll to the bottom so newly-appended lines are
        // always visible. A large negative y is clamped to the
        // post-render max during the next paint — no need to know
        // the actual content height.
        self.log_scroll
            .set_offset(gpui::point(px(0.0), px(-100_000.0)));
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme::bench_black())
            .text_color(theme::text_primary())
            .child(sidebar::render(self.selected, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(header::render(&self.connection))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .child(panes::render(self.selected, cx)),
                    )
                    .child(log::render(&self.log_lines, &self.log_scroll)),
            )
    }
}

/// UTC-clock HH:MM:SS. Cheap; no chrono dep.
fn now_hms() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
}
