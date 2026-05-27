//! GPUI frontend. Compiled only with the `gui` feature.

use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, ScrollHandle, ScrollStrategy, Styled,
    Subscription, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowDecorations, WindowOptions, actions, div, px,
};
use gpui_component::{
    Root, Theme, ThemeMode, TitleBar,
    input::{InputEvent, InputState},
    resizable::{resizable_panel, v_resizable},
};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ch341::Ch341;
use crate::ops::{self, Diagnosis, ProgressSink};
use crate::prefs::Prefs;

// Global window-level keyboard actions. `actions!` generates
// zero-sized types implementing `Action`; key bindings dispatch by
// type. `None` context means the binding fires anywhere in the
// window's dispatch chain.
actions!(
    etch341,
    [FocusFind, FindNextAction, FindPrevAction, CopyHexSelection]
);

// The search-pattern parsing, string extraction, and byte-level
// match logic live in the shared `crate::inspect` module so the CLI
// can use the same code. Re-exported here for the convenience of
// the rest of the GUI module.
pub use crate::inspect::{byte_match_ci, extract_strings, parse_hex_needle};

mod header;
mod log;
mod panes;
mod sidebar;
mod theme;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(|cx: &mut App| {
        gpui_component::init(cx);
        // Force gpui-component into dark mode regardless of the OS
        // appearance — etch341's chrome (sidebar, panes, log, hex)
        // is dark-only by design. Without this the gpui-component
        // widgets we embed (`TitleBar`, `v_resizable`, the
        // `Input` used by hex-search) paint with the *light* theme
        // and stand out as bright tiles against our dark window.
        // PortFinder mirrors the OS via `apply_system_theme`; we
        // just pin the mode because there's no light variant to
        // switch into.
        Theme::change(ThemeMode::Dark, None, cx);

        // Standard hex-editor shortcuts. GPUI treats `cmd-` and
        // `ctrl-` as distinct chords — `cmd-` only fires for the
        // macOS Command key, `ctrl-` for the Control key. We gate
        // each set to its native platform: `cmd-*` on macOS only,
        // `ctrl-*` on Windows + Linux only. Without this gating,
        // Ctrl+C on macOS would intercept the Hex-copy action even
        // though Mac users expect Cmd+C — and Ctrl+C in any GUI
        // app on macOS is non-idiomatic enough that hijacking it
        // would shadow whatever else the user had in mind.
        //
        // The `Some("Input")` scope overrides gpui-component's
        // in-Input Search action so the Find shortcut still jumps
        // to our Find field even when another Input has focus.
        // Cmd+C / Ctrl+C in the Hex pane copies the selected
        // bytes; the handler early-returns when an Input has
        // focus, so the gpui-component Input's own
        // copy-to-clipboard still works inside the search field.
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-f", FocusFind, None),
            KeyBinding::new("cmd-f", FocusFind, Some("Input")),
            KeyBinding::new("cmd-g", FindNextAction, None),
            KeyBinding::new("cmd-shift-g", FindPrevAction, None),
            KeyBinding::new("cmd-c", CopyHexSelection, None),
        ]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([
            KeyBinding::new("ctrl-f", FocusFind, None),
            KeyBinding::new("ctrl-f", FocusFind, Some("Input")),
            KeyBinding::new("ctrl-g", FindNextAction, None),
            KeyBinding::new("ctrl-shift-g", FindPrevAction, None),
            KeyBinding::new("ctrl-c", CopyHexSelection, None),
        ]);

        // Load prefs once up front so we can honour
        // `restore_window_bounds` at open time. Loaded again inside
        // the `on_window_should_close` handler to avoid persisting a
        // stale snapshot if the user toggled the pref mid-session.
        let prefs_at_open = Prefs::load();
        let bounds = prefs_at_open
            .restore_window_bounds
            .then(|| {
                prefs_at_open.window_bounds.map(|g| Bounds {
                    origin: gpui::point(px(g.x), px(g.y)),
                    size: gpui::size(px(g.width), px(g.height)),
                })
            })
            .flatten()
            .unwrap_or_else(|| Bounds::centered(None, gpui::size(px(1200.0), px(800.0)), cx));
        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // `TitleBar::title_bar_options()` provides
                // `appears_transparent: true` plus the
                // traffic-light position that matches gpui-
                // component's default 34px titlebar height. We
                // used to override `traffic_light_position` to
                // `(16, 16)` (copied from Baudrun, where skins set
                // a taller titlebar), but that left the lights
                // bottom-aligned inside our default-height bar.
                // PortFinder just takes the defaults; we follow
                // suit. `title` is set so the OS taskbar / dock /
                // window-list still labels the window even though
                // the widget draws no visible text by default.
                titlebar: Some(TitlebarOptions {
                    title: Some("etch341".into()),
                    ..TitleBar::title_bar_options()
                }),
                app_id: Some("etch341".into()),
                // Force client-side decorations. Without this gpui
                // defaults to `Server`, which KDE Plasma's KWin
                // honours by drawing its own server-side title bar
                // *on top of* our gpui-component TitleBar — dual
                // titlebars stacked on every KDE install. Mutter
                // (GNOME) ignores the protocol and always does CSD,
                // so the bug only manifests under KWin, but the fix
                // is universal. No-op on macOS / Windows. Pairs with
                // `TitleBar::title_bar_options()` above — they're
                // intentionally set together as gpui-component's own
                // example apps do.
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            },
            |window, cx| {
                // Widen the resize hit-test margin from the gpui
                // default (which is effectively 0 on Wayland CSD
                // because the compositor refuses xdg-decoration).
                // Without this the diagonal/edge resize zones are an
                // unreachably-thin one-pixel strip on GNOME-Wayland.
                // No-op on macOS / Windows / X11.
                window.set_client_inset(px(10.0));
                // Persist window bounds on close iff the user opted
                // in via Settings → "Restore window position on
                // startup". Reloads from disk so a fresh toggle
                // (or a SPI-speed change made earlier in the session
                // and saved through `Prefs::save`) survives without
                // us holding a stale clone here.
                window.on_window_should_close(cx, |window, _cx| {
                    let mut prefs = Prefs::load();
                    if prefs.restore_window_bounds {
                        let b = window.bounds();
                        prefs.window_bounds = Some(crate::prefs::WindowGeometry {
                            x: f32::from(b.origin.x),
                            y: f32::from(b.origin.y),
                            width: f32::from(b.size.width),
                            height: f32::from(b.size.height),
                        });
                        if let Err(err) = prefs.save() {
                            eprintln!("etch341: save window bounds: {err}");
                        }
                    }
                    true
                });
                let view = cx.new(|cx| AppView::new(window, cx));
                // Give the root view focus so window-scoped key
                // bindings (None context) have a dispatch path. Without
                // this, Cmd+F etc. would only fire after the user
                // clicked into some focusable widget first.
                view.read(cx).focus_handle.clone().focus(window, cx);
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
    Status,
    Sfdp,
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
    /// Last-read SR1/SR2/SR3 bytes from the Status pane's "Read"
    /// button. `None` before the first read or after the user
    /// reset state. Held as the raw struct so the pane render
    /// shares the same decoded view (`StatusRegisters::wip` etc.)
    /// the CLI's `ops::status` uses.
    pub status_regs: Option<crate::spi::StatusRegisters>,
    /// Last SFDP dump + parsed result from the SFDP pane's "Read"
    /// button. Both the raw bytes (for the hex preview) and the
    /// decoded `Sfdp` are kept; the pane render formats both.
    pub sfdp_dump: Option<(Vec<u8>, crate::sfdp::Sfdp)>,
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
    /// Arc so the uniform_list closure (which must be `'static`) can
    /// own a cheap clone without copying the whole buffer.
    pub hex_bytes: Option<Arc<Vec<u8>>>,
    /// Strings extracted from `hex_bytes` and cached at load time.
    /// Recomputing per-render was a bottleneck on large files
    /// (multi-MB chip dumps). Cache invalidated on file change.
    pub hex_strings: Option<Arc<Vec<(usize, String)>>>,
    /// Set of byte positions that are part of a hex-view-search match.
    /// Recomputed on file change and on every search-term change.
    pub hex_byte_matches: Arc<HashSet<usize>>,
    /// Sorted start positions of each match, in file order. Used to
    /// step between matches with Find next / prev.
    pub hex_match_starts: Vec<usize>,
    /// Index into `hex_match_starts` for the currently-focused match.
    /// `None` until the user presses Enter / clicks next/prev.
    pub hex_current_match: Option<usize>,
    /// Preserves the hex view's scroll position across re-renders
    /// (filter changes, toggling Strings, etc.).
    pub hex_scroll: UniformListScrollHandle,
    /// Line index to highlight in the hex view (set by
    /// `jump_to_hex_offset`). Sticky — stays until the next jump.
    pub hex_highlight_line: Option<usize>,
    /// Same idea but for the strings list. Separate handle so the
    /// two views can scroll independently.
    pub strings_scroll: UniformListScrollHandle,
    /// Toggle between raw hex dump (false) and extracted-strings view (true).
    pub hex_show_strings: bool,
    /// Byte-selection range in the hex view: `(anchor, extent)`, both
    /// inclusive offsets into `hex_bytes`. Stored unnormalized so
    /// shift-click can extend in either direction relative to the
    /// anchor; callers normalize via `selection_range()`.
    pub hex_selection: Option<(usize, usize)>,
    /// True between mouse-down and mouse-up inside the hex view.
    /// Gates drag-extend so `on_mouse_move` only extends while the
    /// button is held.
    pub hex_selecting: bool,
    /// Live-updated filter for the Strings list. Synced from the Input
    /// widget via the subscription stored in `_subscriptions`.
    pub hex_search_term: String,
    /// Managed-state entity for the unified Find input. Live typing
    /// drives highlight + filter; Enter dispatches to jump-or-find.
    pub hex_search_state: Entity<InputState>,
    /// Keeps subscriptions alive for as long as AppView exists. Drop
    /// the subscription = dead callback = stale UI.
    _subscriptions: Vec<Subscription>,
    /// Shared with the background ops task; rendered in the session
    /// header by `header::render`.
    pub progress: Arc<SharedProgress>,
    /// Persistent user prefs (SPI speed, future settings).
    pub prefs: Prefs,
    /// Focus handle for the root view. Without this, key bindings
    /// with `None` context never fire — gpui's dispatcher walks the
    /// focus chain, and a window with nothing focused has an empty
    /// chain. Tracking focus on the root div + focusing this handle
    /// on startup gives global shortcuts (Cmd+F, Cmd+G) somewhere to
    /// land.
    pub focus_handle: FocusHandle,
}

impl AppView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let hex_search_state = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                "Find: text or hex bytes (e.g. NVIDIA, 55 AA), or 0xOFFSET, Enter to jump",
            )
        });
        // Bridge the Input's Change events into our own `hex_search_term`
        // String so panes::render can filter without needing the Entity.
        // `subscribe_in` (not plain `subscribe`) is the canonical way to
        // handle widget events — plain `subscribe` routes the callback
        // through gpui's async-context path, which panics with
        // "RefCell already borrowed" the first time an Input event fires.
        let sub = cx.subscribe_in(
            &hex_search_state,
            window,
            |this: &mut AppView, state, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    this.hex_search_term = state.read(cx).value().to_string();
                    this.recompute_hex_matches();
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => {
                    this.find_enter(cx);
                }
                _ => {}
            },
        );
        Self {
            selected: Pane::Detect,
            connection: Connection::Disconnected,
            status_regs: None,
            sfdp_dump: None,
            log_lines: vec![LogLine {
                timestamp: now_hms(),
                text: "etch341 ready. Plug in a CH341A and click Detect chip.".into(),
            }],
            log_scroll: ScrollHandle::new(),
            erase_armed: false,
            write_armed: false,
            write_input_path: None,
            verify_input_path: None,
            hex_input_path: None,
            hex_bytes: None,
            hex_strings: None,
            hex_byte_matches: Arc::new(HashSet::new()),
            hex_match_starts: Vec::new(),
            hex_current_match: None,
            hex_scroll: UniformListScrollHandle::new(),
            strings_scroll: UniformListScrollHandle::new(),
            hex_highlight_line: None,
            hex_show_strings: false,
            hex_selection: None,
            hex_selecting: false,
            hex_search_term: String::new(),
            hex_search_state,
            _subscriptions: vec![sub],
            progress: Arc::new(SharedProgress::default()),
            prefs: Prefs::load(),
            focus_handle: cx.focus_handle(),
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

    /// Open the directory containing `prefs.toml` in the OS file
    /// manager. Best-effort — if `$HOME` isn't set (no prefs path),
    /// or the platform helper can't be spawned, we log the failure
    /// and move on. Done as a `Command::spawn` (not `output()`) so
    /// the GUI doesn't block on file-manager startup.
    pub fn open_prefs_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = Prefs::path() else {
            self.push_log("open prefs folder: $HOME not set".to_string());
            cx.notify();
            return;
        };
        let Some(dir) = path.parent() else {
            self.push_log(format!(
                "open prefs folder: no parent for {}",
                path.display()
            ));
            cx.notify();
            return;
        };
        // Per-OS file-manager invocation. `open` (macOS), `explorer`
        // (Windows), and `xdg-open` (Linux freedesktop spec) all
        // accept a directory and open it in the default file
        // browser. No third-party crate dep — the surface area is
        // three lines of platform-gated code.
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "windows")]
        let cmd = "explorer";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let cmd: &str = "";

        if cmd.is_empty() {
            self.push_log("open prefs folder: unsupported platform".to_string());
            cx.notify();
            return;
        }
        match std::process::Command::new(cmd).arg(dir).spawn() {
            Ok(_) => self.push_log(format!("Opened {}", dir.display())),
            Err(e) => self.push_log(format!("open prefs folder: {e}")),
        }
        cx.notify();
    }

    /// Flip the "restore window position on startup" toggle. The
    /// actual save happens inside the window-close handler in
    /// `gui::run` — turning it off here simply means the next close
    /// won't snapshot bounds (and any previously-saved
    /// `window_bounds` is left in the file but ignored on next
    /// launch).
    pub fn toggle_restore_window_bounds(&mut self, cx: &mut Context<Self>) {
        self.prefs.restore_window_bounds = !self.prefs.restore_window_bounds;
        let state = if self.prefs.restore_window_bounds {
            "on"
        } else {
            "off"
        };
        match self.prefs.save() {
            Ok(()) => self.push_log(format!("Restore window position: {state} (saved)")),
            Err(e) => self.push_log(format!(
                "Restore window position: {state} (save failed: {e})"
            )),
        }
        cx.notify();
    }

    /// Spawn a foreground task that calls `cx.notify()` every 100ms
    /// for as long as `progress.active` is true. Each `start_*` op
    /// kicks off a poller; the loop exits one tick after the work
    /// completes (so the final 100% state lands before going away).
    fn spawn_progress_poller(&self, cx: &mut Context<Self>) {
        let progress = self.progress.clone();
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

    /// Open the OS folder picker to choose where Read pane dumps
    /// should land. Saved to `prefs.read_output_dir`; cleared back
    /// to `None` (use `$HOME` fallback) by passing nothing the
    /// picker rejects. Deferred via `cx.spawn` so the dialog
    /// doesn't block the foreground render — same panic-avoidance
    /// reason as `pick_hex_file`.
    pub fn pick_read_output_dir(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.read_output_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new();
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_folder().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Read save location: {}", path.display()));
                this.prefs.read_output_dir = Some(path);
                let _ = this.prefs.save();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Open the OS file picker to choose a binary to write to the chip.
    /// Deferred via cx.spawn — see `pick_hex_file` for the panic-avoidance
    /// rationale. Remembers the parent dir as `last_write_dir` so the
    /// next pick lands in the same place.
    pub fn pick_write_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.write_input_path = Some(path);
                this.write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Refresh `hex_byte_matches` + `hex_first_match` from the current
    /// `hex_bytes` and `hex_search_term`. Called from the search
    /// subscription and after loading a new file.
    fn recompute_hex_matches(&mut self) {
        self.hex_current_match = None;
        let Some(bytes) = self.hex_bytes.as_ref() else {
            self.hex_byte_matches = Arc::new(HashSet::new());
            self.hex_match_starts = Vec::new();
            return;
        };
        let pattern = parse_hex_needle(&self.hex_search_term);
        if pattern.is_empty() || bytes.len() < pattern.len() {
            self.hex_byte_matches = Arc::new(HashSet::new());
            self.hex_match_starts = Vec::new();
            return;
        }
        let pat_len = pattern.len();
        let mut set = HashSet::new();
        let mut starts = Vec::new();
        for i in 0..=bytes.len() - pat_len {
            let hit = (0..pat_len).all(|j| byte_match_ci(bytes[i + j], pattern[j]));
            if hit {
                starts.push(i);
                for j in 0..pat_len {
                    set.insert(i + j);
                }
            }
        }
        self.hex_byte_matches = Arc::new(set);
        self.hex_match_starts = starts;
    }

    /// PressEnter on the find input. `0x...` → explicit offset jump;
    /// anything else steps to the next match (wraps from `None` cursor
    /// to first match).
    pub fn find_enter(&mut self, cx: &mut Context<Self>) {
        let raw = self.hex_search_state.read(cx).value().to_string();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.starts_with("0x") || trimmed.starts_with("0X") {
            self.jump_via_input(trimmed, cx);
            return;
        }
        self.find_next(cx);
    }

    /// Advance the find cursor to the next match (wrap at the end).
    /// Fresh search → lands on match 0.
    pub fn find_next(&mut self, cx: &mut Context<Self>) {
        let total = self.hex_match_starts.len();
        if total == 0 {
            self.push_log("Find: no matches".into());
            cx.notify();
            return;
        }
        let next_idx = match self.hex_current_match {
            Some(i) => (i + 1) % total,
            None => 0,
        };
        self.hex_current_match = Some(next_idx);
        let offset = self.hex_match_starts[next_idx];
        self.push_log(format!(
            "Find: match {}/{} at 0x{:X}",
            next_idx + 1,
            total,
            offset
        ));
        self.jump_to_hex_offset(offset, cx);
    }

    /// Step the find cursor back by one match (wrap at the start).
    /// Fresh search → lands on the last match.
    pub fn find_prev(&mut self, cx: &mut Context<Self>) {
        let total = self.hex_match_starts.len();
        if total == 0 {
            self.push_log("Find: no matches".into());
            cx.notify();
            return;
        }
        let prev_idx = match self.hex_current_match {
            Some(i) => (i + total - 1) % total,
            None => total - 1,
        };
        self.hex_current_match = Some(prev_idx);
        let offset = self.hex_match_starts[prev_idx];
        self.push_log(format!(
            "Find: match {}/{} at 0x{:X}",
            prev_idx + 1,
            total,
            offset
        ));
        self.jump_to_hex_offset(offset, cx);
    }

    /// Swap the Hex pane between raw-bytes view and extracted-strings view.
    pub fn set_hex_strings_mode(&mut self, show_strings: bool, cx: &mut Context<Self>) {
        self.hex_show_strings = show_strings;
        cx.notify();
    }

    /// Wired to clicks on string-list rows: switch to Hex view and
    /// scroll the hex `uniform_list` so the byte at `offset` is
    /// centered in the viewport. Scroll position takes effect on the
    /// next render (after `cx.notify()` triggers it).
    /// Parse the `Jump:` input value and dispatch to
    /// `jump_to_hex_offset`. Accepts `0xFA00`, `0XFA00`, or bare hex
    /// `FA00`. Logs an explanatory error on bad input or out-of-range
    /// offsets — leaves the input intact so the user can correct it.
    pub fn jump_via_input(&mut self, raw: &str, cx: &mut Context<Self>) {
        // Strip ALL whitespace so `0x FA 00` and `55 AA` both parse.
        // The address is logically the concatenation of the hex digits
        // typed; users sometimes type a byte sequence with spaces.
        let condensed: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
        if condensed.is_empty() {
            return;
        }
        let stripped = condensed
            .strip_prefix("0x")
            .or_else(|| condensed.strip_prefix("0X"))
            .unwrap_or(&condensed);
        let parsed = usize::from_str_radix(stripped, 16);
        match parsed {
            Ok(offset) => {
                let size = self.hex_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
                if size == 0 {
                    self.push_log("Jump: no file loaded".into());
                    cx.notify();
                    return;
                }
                if offset >= size {
                    self.push_log(format!(
                        "Jump: 0x{:X} is past end of file (size 0x{:X})",
                        offset, size
                    ));
                    cx.notify();
                    return;
                }
                self.push_log(format!("Jump to 0x{:X}", offset));
                self.jump_to_hex_offset(offset, cx);
            }
            Err(_) => {
                self.push_log(format!(
                    "Jump: \u{201C}{}\u{201D} isn't a valid hex offset",
                    raw
                ));
                cx.notify();
            }
        }
    }

    pub fn jump_to_hex_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.hex_show_strings = false;
        let line = offset / 16;
        // Land the highlighted line a few rows below the top of the
        // viewport so the user sees a bit of context above it. Using
        // `Top` with an offset is more predictable than `Center`,
        // which depends on measured item height and was drifting by
        // a few lines.
        self.hex_scroll
            .scroll_to_item_with_offset(line, ScrollStrategy::Top, 3);
        self.hex_highlight_line = Some(line);
        cx.notify();
    }

    /// Selection as a normalized `(lo, hi)` inclusive range. Returns
    /// `None` if nothing is selected or `hex_bytes` is empty.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let (a, b) = self.hex_selection?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// Mouse-down on a hex byte cell. Anchors a new selection — or,
    /// if shift is held and a selection already exists, extends it
    /// without moving the anchor. Sets `hex_selecting` so subsequent
    /// drag-moves can extend.
    pub fn begin_select(&mut self, byte: usize, shift: bool, cx: &mut Context<Self>) {
        if shift {
            if let Some((anchor, _)) = self.hex_selection {
                self.hex_selection = Some((anchor, byte));
            } else {
                self.hex_selection = Some((byte, byte));
            }
        } else {
            self.hex_selection = Some((byte, byte));
        }
        self.hex_selecting = true;
        cx.notify();
    }

    /// Mouse-move while the left button is held inside the hex view.
    /// No-op outside an active drag.
    pub fn extend_select(&mut self, byte: usize, cx: &mut Context<Self>) {
        if !self.hex_selecting {
            return;
        }
        if let Some((anchor, _)) = self.hex_selection {
            self.hex_selection = Some((anchor, byte));
            cx.notify();
        }
    }

    /// Mouse-up — drag is over, but the selection persists for Cmd+C.
    pub fn end_select(&mut self, _cx: &mut Context<Self>) {
        self.hex_selecting = false;
    }

    /// Copy the current hex-view selection to the system clipboard as
    /// space-separated upper-case hex (e.g. "DE AD BE EF"). Gated on
    /// the Hex pane being visible — Cmd+C in any other context (Write
    /// pane, focused Input, etc.) is a no-op here so the OS / Input
    /// widget's own copy still works.
    pub fn copy_hex_selection(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        let Some((lo, hi)) = self.selection_range() else {
            return;
        };
        let Some(bytes) = self.hex_bytes.as_ref() else {
            return;
        };
        let end = (hi + 1).min(bytes.len());
        if lo >= end {
            return;
        }
        let slice = &bytes[lo..end];
        let mut s = String::with_capacity(slice.len() * 3);
        for (i, b) in slice.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(&format!("{:02X}", b));
        }
        cx.write_to_clipboard(ClipboardItem::new_string(s));
        self.push_log(format!(
            "Copied {} byte{} from 0x{:08X}",
            end - lo,
            if end - lo == 1 { "" } else { "s" },
            lo
        ));
    }

    /// Open the file picker, load the chosen file into memory, and
    /// stash for the Hex pane to render. Files up to a few MB are fine
    /// to hold in memory; the renderer caps the visible window separately.
    ///
    /// **Deferred via cx.spawn:** NSOpenPanel (and its Linux/Windows
    /// equivalents) pump their own modal event loop. If we open the
    /// dialog synchronously from inside a click handler, AppKit
    /// dispatches pending events from the focused Input back into a
    /// gpui context that's still holding our `&mut AppView` borrow,
    /// and gpui panics with "RefCell already borrowed". Running on a
    /// foreground async task lets the click-handler borrow drop first.
    pub fn pick_hex_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_hex_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            let read_result = std::fs::read(&path);
            weak.update(cx, |this, cx| {
                match read_result {
                    Ok(bytes) => {
                        let bytes_arc = Arc::new(bytes);
                        let strings = extract_strings(&bytes_arc, 4);
                        this.push_log(format!(
                            "Loaded hex view: {} ({} bytes, {} strings)",
                            path.display(),
                            bytes_arc.len(),
                            strings.len()
                        ));
                        if let Some(parent) = path.parent() {
                            this.prefs.last_hex_dir = Some(parent.to_path_buf());
                            let _ = this.prefs.save();
                        }
                        this.hex_input_path = Some(path);
                        this.hex_bytes = Some(bytes_arc);
                        this.hex_strings = Some(Arc::new(strings));
                        this.recompute_hex_matches();
                    }
                    Err(e) => this.push_log(format!("Hex view load failed: {e}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn pick_verify_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_verify_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Flash dumps", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for verify: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_verify_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.verify_input_path = Some(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
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
            self.push_log("⚠ Write armed: click again to confirm".into());
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
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
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
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
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
        self.push_log("→ detect".to_string());
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
                            "Unknown JEDEC 0x{}: add an entry to chips.toml",
                            result.jedec_string()
                        ));
                        self.connection = Connection::NoChip;
                    }
                    Diagnosis::MisoStuckLow => {
                        self.push_log(
                            "MISO stuck low: target board contention (lift chip or pin 8)".into(),
                        );
                        self.connection = Connection::NoChip;
                    }
                    Diagnosis::MisoFloatsHigh => {
                        self.push_log(
                            "MISO floats high: no chip detected (check clip, VCC, pin 1)".into(),
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
        // Auto-scroll so the newest line is visible. `scroll_to_bottom`
        // sets a paint-time flag the scroll element honours *after*
        // the new line has been laid out — the previous approach
        // (`set_offset(point(0, -100_000))`) clamped against the
        // *current* (pre-new-line) content height, so the viewport
        // could land one line short on rapid append.
        self.log_scroll.scroll_to_bottom();
    }

    /// Fire a background read of the whole chip to a timestamped file
    /// in $HOME. The blocking USB+SPI work runs on
    /// `cx.background_executor()` so the GUI stays responsive; on
    /// completion the foreground updates the log + connection state.
    pub fn start_read(&mut self, cx: &mut Context<Self>) {
        let path = read_output_path(&self.prefs);
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
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
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
            self.push_log("⚠ Erase armed: click again to confirm".into());
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
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
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
    /// Read SR1/SR2/SR3 in the background and stash the result in
    /// `self.status_regs` for the Status pane to render. Mirrors
    /// the `etch341 sr` CLI subcommand. No progress bar — the read
    /// is three single-byte SPI ops, much faster than the polling
    /// interval that drives `SharedProgress`.
    pub fn start_read_status(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ status regs".into());
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    // Same JEDEC-first guard as `ops::status` —
                    // bail with a friendly message instead of
                    // showing a "decoded" 0xFF as protected state.
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    match detect.diagnosis {
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                        _ => {}
                    }
                    crate::spi::read_all_status(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        this.status_regs = Some(regs);
                        this.push_log(format!(
                            "Status OK: SR1=0x{:02X} SR2=0x{:02X} SR3=0x{:02X}",
                            regs.sr1, regs.sr2, regs.sr3
                        ));
                    }
                    Err(err) => {
                        this.push_log(format!("Status FAIL: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read 256 bytes of SFDP off the chip and stash the parsed
    /// result in `self.sfdp_dump` for the SFDP pane to render.
    /// Mirrors the `etch341 sfdp` CLI. Same JEDEC-first guard as
    /// the Status read so an MISO-stuck-low / floats-high path
    /// gets a friendly log message instead of a 256-byte all-FF
    /// "decoded" dump.
    pub fn start_read_sfdp(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ sfdp".into());
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let detect = ops::run_detect(&mut ch).map_err(|e| format!("detect: {e}"))?;
                    match detect.diagnosis {
                        Diagnosis::MisoStuckLow => {
                            return Err("MISO stuck low (target board contention)".into());
                        }
                        Diagnosis::MisoFloatsHigh => {
                            return Err("MISO floats high (no chip / HOLD# grounded)".into());
                        }
                        _ => {}
                    }
                    let data =
                        crate::spi::read_sfdp(&mut ch, 0, 256).map_err(|e| format!("{e}"))?;
                    let parsed = crate::sfdp::parse(&data);
                    Ok::<_, String>((data, parsed))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((data, parsed)) => {
                        let summary = if let Some(b) = &parsed.bfpt {
                            format!(
                                "SFDP OK: rev {}.{}, {} bytes, page {}",
                                parsed.header.major_rev,
                                parsed.header.minor_rev,
                                b.size_bytes,
                                b.page_size
                            )
                        } else if parsed.header.valid {
                            format!(
                                "SFDP OK: rev {}.{} (BFPT missing)",
                                parsed.header.major_rev, parsed.header.minor_rev,
                            )
                        } else {
                            "SFDP: chip didn't return 'SFDP' magic (no JESD216 support)".to_string()
                        };
                        this.sfdp_dump = Some((data, parsed));
                        this.push_log(summary);
                    }
                    Err(err) => {
                        this.push_log(format!("SFDP FAIL: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

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
                            // JEDEC isn't in the bundled DB; fall back to
                            // SFDP so a brand-new chip can still be
                            // read/written/erased/verified using its
                            // self-described parameters. If SFDP isn't
                            // available either, surface the
                            // ChipNotRecognized condition with both
                            // escape hatches in the message.
                            let jedec = detect.jedec_string();
                            match ops::synthesize_from_sfdp(&mut ch, &jedec) {
                                Ok(Some(c)) => c,
                                _ => {
                                    return Err(format!(
                                        "unknown JEDEC 0x{jedec} and chip has no SFDP; add to chips.toml or pass --chip"
                                    ));
                                }
                            }
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

/// Pick a filename for the next read dump. Uses the directory the
/// user set in Settings (`prefs.read_output_dir`) if any, otherwise
/// `$HOME` so the dump persists past reboots. The seconds-since-epoch
/// suffix makes consecutive reads land in distinct files.
fn read_output_path(prefs: &Prefs) -> std::path::PathBuf {
    let dir = prefs.read_output_dir.clone().unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
    });
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    dir.join(format!("etch341-read-{secs}.bin"))
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prefs_path_buf = Prefs::path();
        // Outer column: TitleBar on top, then the existing sidebar +
        // main-column row fills the rest. The TitleBar widget draws
        // the window title text plus min/max/close controls on
        // Windows/Linux (where `appears_transparent` hides the
        // native chrome) and renders transparently on macOS so the
        // native traffic lights show through. Without this widget,
        // the Windows build looks chromeless and unmovable — exactly
        // the symptom the .exe shows on a fresh Windows VM.
        div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bench_black())
            .text_color(theme::text_primary())
            // Window-level action handlers — these fire via the
            // gpui-registered key bindings above. Must stay on the
            // outer root div (the one with `track_focus`) so action
            // dispatch from the focused element bubbles up and finds
            // them.
            .on_action(
                cx.listener(|this: &mut AppView, _: &FocusFind, window, cx| {
                    this.selected = Pane::Hex;
                    this.hex_search_state.update(cx, |state, cx| {
                        state.focus(window, cx);
                    });
                    cx.notify();
                }),
            )
            .on_action(
                cx.listener(|this: &mut AppView, _: &FindNextAction, _, cx| {
                    this.find_next(cx);
                }),
            )
            .on_action(
                cx.listener(|this: &mut AppView, _: &FindPrevAction, _, cx| {
                    this.find_prev(cx);
                }),
            )
            .on_action(
                cx.listener(|this: &mut AppView, _: &CopyHexSelection, _, cx| {
                    this.copy_hex_selection(cx);
                }),
            )
            .child(TitleBar::new())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
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
                            .child({
                                // Settings is a configuration pane, not an
                                // op — there's no progress / pass / fail
                                // log activity to watch while editing it, so
                                // the activity log pane just steals screen
                                // real estate. Render the settings full-
                                // height without the splitter when it's
                                // selected; restore the pane / log split for
                                // every other pane. The hex pane keeps the
                                // log too — file-load events still surface
                                // useful timing / size info there.
                                let pane_inputs = panes::PaneInputs {
                                    erase_armed: self.erase_armed,
                                    write_armed: self.write_armed,
                                    write_path: self.write_input_path.as_deref(),
                                    verify_path: self.verify_input_path.as_deref(),
                                    hex_path: self.hex_input_path.as_deref(),
                                    hex_bytes: self.hex_bytes.clone(),
                                    hex_strings: self.hex_strings.clone(),
                                    hex_byte_matches: self.hex_byte_matches.clone(),
                                    hex_match_total: self.hex_match_starts.len(),
                                    hex_current_match: self.hex_current_match,
                                    hex_scroll: self.hex_scroll.clone(),
                                    strings_scroll: self.strings_scroll.clone(),
                                    hex_highlight_line: self.hex_highlight_line,
                                    hex_show_strings: self.hex_show_strings,
                                    hex_selection: self.selection_range(),
                                    hex_search_term: self.hex_search_term.as_str(),
                                    hex_search_state: &self.hex_search_state,
                                    spi_speed_khz: self.prefs.spi_speed_khz,
                                    restore_window_bounds: self.prefs.restore_window_bounds,
                                    prefs_path: prefs_path_buf.as_deref(),
                                    status_regs: self.status_regs,
                                    read_output_dir: self.prefs.read_output_dir.as_deref(),
                                    sfdp_dump: self.sfdp_dump.as_ref(),
                                };
                                let outer = div().flex_1().min_h(px(0.0)).min_w(px(0.0));
                                if self.selected == Pane::Settings {
                                    outer.child(
                                        div()
                                            .size_full()
                                            .overflow_hidden()
                                            .flex()
                                            .flex_col()
                                            .child(panes::render(self.selected, pane_inputs, cx)),
                                    )
                                } else {
                                    let log_h = self.prefs.log_panel_height.unwrap_or(180.0);
                                    let weak = cx.entity().downgrade();
                                    outer.child(
                                        v_resizable("pane-log-split")
                                            .on_resize(move |state, _, cx| {
                                                let sizes = state.read(cx).sizes().clone();
                                                let Some(log_size) = sizes.get(1).copied() else {
                                                    return;
                                                };
                                                let new_h = f32::from(log_size);
                                                let _ = weak.update(cx, |this, _| {
                                                    if this.prefs.log_panel_height != Some(new_h) {
                                                        this.prefs.log_panel_height = Some(new_h);
                                                        let _ = this.prefs.save();
                                                    }
                                                });
                                            })
                                            .child(resizable_panel().child(
                                                div().overflow_hidden().flex().flex_col().child(
                                                    panes::render(self.selected, pane_inputs, cx),
                                                ),
                                            ))
                                            .child(
                                                // Min was `80.0` originally — under
                                                // ~6 lines of log it stops being
                                                // useful (the top edge clips into
                                                // the most recent activity and the
                                                // user can't even read one full
                                                // message). 120px gives ~9 lines
                                                // of comfortable margin and still
                                                // lets the user shrink it well
                                                // below the default 180px when
                                                // they want more pane real estate.
                                                resizable_panel()
                                                    .size(px(log_h))
                                                    .size_range(px(120.0)..px(500.0))
                                                    .child(log::render(
                                                        &self.log_lines,
                                                        &self.log_scroll,
                                                    )),
                                            ),
                                    )
                                }
                            }),
                    ),
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
