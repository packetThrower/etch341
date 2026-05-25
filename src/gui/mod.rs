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

    /// Fire a background read of the whole chip to a timestamped file
    /// in $HOME. The blocking USB+SPI work runs on
    /// `cx.background_executor()` so the GUI stays responsive; on
    /// completion the foreground updates the log + connection state.
    pub fn start_read(&mut self, cx: &mut Context<Self>) {
        let path = read_output_path();
        self.push_log(format!("→ read → {}", path.display()));
        cx.notify();

        let path_for_task = path.clone();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            return Err(format!(
                                "unknown JEDEC 0x{} — add to chips.toml",
                                detect.jedec_string()
                            ));
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    let size = chip.size_kb.saturating_mul(1024);
                    ops::read(&mut ch, &chip, 0, size, &path_for_task)
                        .map_err(|e| format!("read: {e}"))?;
                    Ok::<_, String>((chip.name, size))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => {
                        this.push_log(format!("Read OK : {size} bytes from {name}"));
                        this.push_log(format!("Saved   : {}", path.display()));
                    }
                    Err(err) => {
                        this.push_log(format!("Read FAIL: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Background-spawn a full-chip blank check. Useful for verifying
    /// that an erase succeeded (`ops::blank_check` returns
    /// `Error::NotBlank { addr, value }` on the first non-FF byte;
    /// the location is included in the error message).
    pub fn start_blank_check(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ blank check".into());
        cx.notify();

        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            return Err(format!(
                                "unknown JEDEC 0x{} — add to chips.toml",
                                detect.jedec_string()
                            ));
                        }
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                    };
                    let size = chip.size_kb.saturating_mul(1024);
                    ops::blank_check(&mut ch, &chip).map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((chip.name, size))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => {
                        this.push_log(format!("Blank OK : all {size} bytes are 0xFF ({name})"));
                    }
                    Err(err) => {
                        this.push_log(format!("Blank FAIL: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// Pick a filename for the next read dump. Lives in $HOME so it persists
/// past reboots; the seconds-since-epoch suffix makes consecutive reads
/// land in distinct files.
fn read_output_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::path::PathBuf::from(home).join(format!("etch341-read-{secs}.bin"))
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
                    // `min_w(0)` overrides flex's default `min-width: auto`
                    // — without it, intrinsic widths of deeply-nested
                    // children (long paragraphs, log lines) push this
                    // column wider than the available viewport, and the
                    // right edge runs off-window. With `min_w(0)`, flex
                    // shrink obeys the parent's calculated width and
                    // wrapping kicks in for descendants that have
                    // `whitespace_normal`.
                    .min_w(px(0.0))
                    .flex()
                    .flex_col()
                    .child(header::render(&self.connection))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .min_w(px(0.0))
                            .overflow_hidden()
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
