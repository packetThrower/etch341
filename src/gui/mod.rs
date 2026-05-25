//! GPUI frontend. Compiled only with the `gui` feature.

use gpui::{
    App, AppContext, Bounds, Context, IntoElement, ParentElement, Render, ScrollHandle, Styled,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, px,
};
use gpui_component::{Root, TitleBar};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ch341::Ch341;
use crate::ops::{self, Diagnosis, ProgressSink};
use crate::prefs::Prefs;

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
    Hex,
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

/// Shared progress state between the background ops task and the
/// foreground render. The ops task writes via `GuiSink::update`; a
/// poller task on the foreground polls + calls `cx.notify()` every
/// 100ms while `active` is true so the session header re-renders
/// with the latest values.
#[derive(Default)]
pub struct SharedProgress {
    pub current: AtomicU64,
    pub total: AtomicU64,
    pub label: Mutex<String>,
    pub active: AtomicBool,
}

/// `ProgressSink` impl that writes into a `SharedProgress`. The
/// label is set once at construction so we don't burn a Mutex lock
/// in the hot path.
pub struct GuiSink {
    shared: Arc<SharedProgress>,
    label: &'static str,
}

impl GuiSink {
    fn new(shared: Arc<SharedProgress>, label: &'static str) -> Self {
        Self { shared, label }
    }
}

impl ProgressSink for GuiSink {
    fn start(&mut self, total: u64) {
        *self.shared.label.lock().unwrap() = self.label.to_string();
        self.shared.total.store(total, Ordering::Relaxed);
        self.shared.current.store(0, Ordering::Relaxed);
        self.shared.active.store(true, Ordering::Relaxed);
    }
    fn update(&mut self, current: u64) {
        self.shared.current.store(current, Ordering::Relaxed);
    }
    fn finish(&mut self) {
        self.shared.active.store(false, Ordering::Relaxed);
    }
}

pub struct AppView {
    pub selected: Pane,
    pub connection: Connection,
    pub log_lines: Vec<LogLine>,
    /// Persists scroll position across re-renders; required by
    /// `track_scroll(...)` to keep the log from jumping back to the
    /// top whenever a new line is appended.
    pub log_scroll: ScrollHandle,
    /// First click on the Erase button arms it (label/color swap);
    /// the second click within the same pane visit fires the actual
    /// erase. Reset to false when the user navigates away.
    pub erase_armed: bool,
    /// Same two-stage trigger as `erase_armed` but for the Write
    /// pane. Write is destructive (erase-then-program by default),
    /// so it gets the same arm/confirm protection.
    pub write_armed: bool,
    /// File selected via the Write pane's Browse button.
    pub write_input_path: Option<std::path::PathBuf>,
    /// File selected via the Verify pane's Browse button.
    pub verify_input_path: Option<std::path::PathBuf>,
    /// File selected via the Hex pane's Browse button, plus its loaded
    /// contents. Held together so the renderer doesn't re-read the file
    /// on every paint.
    pub hex_input_path: Option<std::path::PathBuf>,
    pub hex_bytes: Option<Vec<u8>>,
    /// Toggle between raw hex dump (false) and extracted-strings view (true).
    pub hex_show_strings: bool,
    /// Shared with the background ops task; rendered in the session
    /// header by `header::render`.
    pub progress: Arc<SharedProgress>,
    /// Persistent user prefs (SPI speed, future settings).
    pub prefs: Prefs,
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
            erase_armed: false,
            write_armed: false,
            write_input_path: None,
            verify_input_path: None,
            hex_input_path: None,
            hex_bytes: None,
            hex_show_strings: false,
            progress: Arc::new(SharedProgress::default()),
            prefs: Prefs::load(),
        }
    }

    /// Persist a new SPI clock setting. Saves to ~/.config/etch341/prefs.toml
    /// immediately; the next op picks up the new value when it opens the
    /// CH341A.
    pub fn set_spi_speed(&mut self, khz: u32, cx: &mut Context<Self>) {
        self.prefs.spi_speed_khz = khz;
        match self.prefs.save() {
            Ok(()) => self.push_log(format!("SPI clock set to {khz} kHz (saved)")),
            Err(e) => self.push_log(format!("SPI clock set to {khz} kHz (save failed: {e})")),
        }
        cx.notify();
    }

    /// Spawn a foreground task that calls `cx.notify()` every 100ms
    /// for as long as `progress.active` is true. Each `start_*` op
    /// kicks off a poller; the loop exits one tick after the work
    /// completes (so the final 100% state lands before going away).
    fn spawn_progress_poller(&self, cx: &mut Context<Self>) {
        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let still_active = progress.active.load(Ordering::Relaxed);
                if weak.update(cx, |_, cx| cx.notify()).is_err() {
                    break; // view has gone away
                }
                if !still_active {
                    break;
                }
            }
        })
        .detach();
    }

    /// Open the OS file picker to choose a binary to write to the chip.
    /// Synchronous: NSOpenPanel runs its own event loop, the GUI is
    /// frozen for the dialog's duration. Acceptable for modal pickers.
    pub fn pick_write_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Flash dumps", &["bin", "rom"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.push_log(format!("Picked for write: {}", path.display()));
            self.write_input_path = Some(path);
            // Re-arm protection on file change.
            self.write_armed = false;
        }
        cx.notify();
    }

    /// Swap the Hex pane between raw-bytes view and extracted-strings view.
    pub fn set_hex_strings_mode(&mut self, show_strings: bool, cx: &mut Context<Self>) {
        self.hex_show_strings = show_strings;
        cx.notify();
    }

    /// Open the file picker, load the chosen file into memory, and
    /// stash for the Hex pane to render. Files up to a few MB are fine
    /// to hold in memory; the renderer caps the visible window separately.
    pub fn pick_hex_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Flash dumps", &["bin", "rom"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    self.push_log(format!(
                        "Loaded hex view: {} ({} bytes)",
                        path.display(),
                        bytes.len()
                    ));
                    self.hex_input_path = Some(path);
                    self.hex_bytes = Some(bytes);
                }
                Err(e) => {
                    self.push_log(format!("Hex view load failed: {e}"));
                }
            }
        }
        cx.notify();
    }

    pub fn pick_verify_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Flash dumps", &["bin", "rom"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.push_log(format!("Picked for verify: {}", path.display()));
            self.verify_input_path = Some(path);
        }
        cx.notify();
    }

    /// Two-stage trigger for write (same shape as `arm_or_fire_erase`).
    pub fn arm_or_fire_write(&mut self, cx: &mut Context<Self>) {
        if self.write_input_path.is_none() {
            self.push_log("Write FAIL: no input file selected".into());
            cx.notify();
            return;
        }
        if self.write_armed {
            self.write_armed = false;
            self.start_write(cx);
        } else {
            self.write_armed = true;
            self.push_log("⚠ Write armed — click again to confirm".into());
            cx.notify();
        }
    }

    /// Background-spawn ops::write with erase-first + verify-after
    /// (matches the CLI's default behaviour). Path must already be set.
    fn start_write(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.write_input_path.clone() else {
            self.push_log("Write FAIL: no input file selected".into());
            cx.notify();
            return;
        };
        self.push_log(format!(
            "→ write {} (erase + program + verify)",
            path.display()
        ));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "write");
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
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
                    ops::write(&mut ch, &chip, &data, 0, true, true, &mut sink)
                        .map_err(|e| format!("write: {e}"))?;
                    Ok::<_, String>((chip.name, data.len()))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, n)) => {
                        this.push_log(format!("Write OK : {n} bytes to {name} (verified)"));
                    }
                    Err(err) => this.push_log(format!("Write FAIL: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Background-spawn ops::verify. Read-only, no confirmation needed.
    pub fn start_verify(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.verify_input_path.clone() else {
            self.push_log("Verify FAIL: no input file selected".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ verify against {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "verify");
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    let chip = match detect.diagnosis {
                        Diagnosis::Known(c) => c,
                        Diagnosis::UnknownChip => {
                            return Err(format!(
                                "unknown JEDEC 0x{} — add to chips.toml",
                                detect.jedec_string()
                            ));
                        }
                        Diagnosis::MisoStuckLow => return Err("MISO stuck low".into()),
                        Diagnosis::MisoFloatsHigh => return Err("MISO floats high".into()),
                    };
                    let mismatches = ops::verify(&mut ch, &chip, &data, 0, &mut sink)
                        .map_err(|e| format!("verify: {e}"))?;
                    Ok::<_, String>((chip.name, data.len(), mismatches))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, n, 0)) => {
                        this.push_log(format!("Verify OK: all {n} bytes match {name}"));
                    }
                    Ok((name, n, mis)) => {
                        this.push_log(format!("Verify FAIL: {mis} of {n} bytes differ ({name})"));
                    }
                    Err(err) => this.push_log(format!("Verify FAIL: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
        self.spawn_progress_poller(cx);

        let path_for_task = path.clone();
        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "read");
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
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
                    ops::read(&mut ch, &chip, 0, size, &path_for_task, &mut sink)
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

    /// Two-stage destructive trigger for full-chip erase. First click
    /// flips `erase_armed`; the button visually re-renders (red text,
    /// new label). Second click within the same pane visit fires the
    /// real erase. Navigating away resets the arm state via the
    /// sidebar's pane-change handler.
    pub fn arm_or_fire_erase(&mut self, cx: &mut Context<Self>) {
        if self.erase_armed {
            self.erase_armed = false;
            self.start_erase(cx);
        } else {
            self.erase_armed = true;
            self.push_log("⚠ Erase armed — click again to confirm".into());
            cx.notify();
        }
    }

    /// Background-spawn the actual full-chip erase. Same shape as
    /// `start_read` / `start_blank_check`; ops::erase_chip handles
    /// the WREN → 0xC7 → poll WIP loop. Typical durations: ~30s for
    /// a 4 MB chip, several minutes for 16 MB+.
    fn start_erase(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ erase chip starting (may take 30s–minutes)".into());
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "erase");
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
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
                    ops::erase_chip(&mut ch, &chip, &mut sink)
                        .map_err(|e| format!("erase: {e}"))?;
                    Ok::<_, String>(chip.name)
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(name) => {
                        this.push_log(format!("Erase OK : {name} (chip is now blank)"));
                    }
                    Err(err) => {
                        this.push_log(format!("Erase FAIL: {err}"));
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
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut sink = GuiSink::new(progress, "blank");
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
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
                    ops::blank_check(&mut ch, &chip, &mut sink).map_err(|e| format!("{e}"))?;
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
        let prefs_path_buf = Prefs::path();
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
                    .child(header::render(&self.connection, &self.progress))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.0))
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .child(panes::render(
                                self.selected,
                                panes::PaneInputs {
                                    erase_armed: self.erase_armed,
                                    write_armed: self.write_armed,
                                    write_path: self.write_input_path.as_deref(),
                                    verify_path: self.verify_input_path.as_deref(),
                                    hex_path: self.hex_input_path.as_deref(),
                                    hex_bytes: self.hex_bytes.as_deref(),
                                    hex_show_strings: self.hex_show_strings,
                                    spi_speed_khz: self.prefs.spi_speed_khz,
                                    prefs_path: prefs_path_buf.as_deref(),
                                },
                                cx,
                            )),
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
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}
