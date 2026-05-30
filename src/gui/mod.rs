//! GPUI frontend. Compiled only with the `gui` feature.

use gpui::{
    App, AppContext, Bounds, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, ScrollHandle, ScrollStrategy, SharedString,
    Styled, Subscription, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowDecorations, WindowHandle, WindowOptions, actions, div, px,
};
use gpui_component::{
    Root, Theme, ThemeMode, TitleBar,
    input::{InputEvent, InputState},
    resizable::{resizable_panel, v_resizable},
    select::SelectState,
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
    [
        FocusFind,
        FindNextAction,
        FindPrevAction,
        CopyHexSelection,
        HexZoomIn,
        HexZoomOut,
        HexZoomReset,
    ]
);

/// Best-effort topmost-visible row for a uniform list. Returns 0 if
/// neither signal is available (fresh handle, never rendered).
///
/// Pinned gpui hides `logical_scroll_top_index` behind a `cfg(test)`
/// gate, and on the base handle `top_item()` returns 0 for
/// uniform_list because the latter virtualises its own children and
/// never populates the base handle's `child_bounds`. So we compute
/// the index from the raw pixel offset and a caller-supplied row
/// height — that's the same height uniform_list uses to lay out
/// rows, so the conversion is exact.
///
/// Deferred scrolls (set by `scroll_to_item*` and consumed by the
/// next render) shadow the offset for one frame, so check those
/// first.
fn uniform_list_top_index(handle: &UniformListScrollHandle, row_height_px: f32) -> usize {
    let state = handle.0.borrow();
    if let Some(deferred) = state.deferred_scroll_to_item.as_ref() {
        return deferred.item_index;
    }
    if row_height_px <= 0.0 {
        return 0;
    }
    let offset_y: f32 = state.base_handle.offset().y.into();
    ((-offset_y / row_height_px).max(0.0)) as usize
}

/// Push the current accent into the embedded gpui-component theme so
/// its widgets (RadioGroup dots, Input focus rings) track our accent
/// instead of staying on the library's default blue. Reads
/// `theme::accent*`, so call after `theme::set_accent_hex`.
fn apply_accent_to_component_theme(cx: &mut App) {
    let t = Theme::global_mut(cx);
    t.primary = theme::accent();
    t.primary_hover = theme::accent_hover();
    t.primary_active = theme::accent_active();
    // The on-primary color (e.g. the RadioGroup checkmark) follows
    // the same luma-based dark/light pick so it stays legible on a
    // light accent.
    t.primary_foreground = theme::accent_foreground();
}

// The search-pattern parsing, string extraction, and byte-level
// match logic live in the shared `crate::inspect` module so the CLI
// can use the same code. Re-exported here for the convenience of
// the rest of the GUI module.
pub use crate::inspect::{byte_match_ci, extract_strings, parse_hex_needle};

mod chipdb_browser;
mod header;
mod log;
mod panes;
mod sidebar;
mod theme;
pub mod updater;

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
        // Both `cmd-=` and `cmd-+` map to "zoom in" — `=` is what the
        // user actually presses on most keyboards (plus sign sits
        // above `=`, requiring shift), but a Dvorak / international
        // layout may bind `+` directly. Zed and most browsers bind
        // both. Same for ctrl on non-macOS.
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-f", FocusFind, None),
            KeyBinding::new("cmd-f", FocusFind, Some("Input")),
            KeyBinding::new("cmd-g", FindNextAction, None),
            KeyBinding::new("cmd-shift-g", FindPrevAction, None),
            KeyBinding::new("cmd-c", CopyHexSelection, None),
            KeyBinding::new("cmd-=", HexZoomIn, None),
            KeyBinding::new("cmd-+", HexZoomIn, None),
            KeyBinding::new("cmd--", HexZoomOut, None),
            KeyBinding::new("cmd-0", HexZoomReset, None),
        ]);
        #[cfg(not(target_os = "macos"))]
        cx.bind_keys([
            KeyBinding::new("ctrl-f", FocusFind, None),
            KeyBinding::new("ctrl-f", FocusFind, Some("Input")),
            KeyBinding::new("ctrl-g", FindNextAction, None),
            KeyBinding::new("ctrl-shift-g", FindPrevAction, None),
            KeyBinding::new("ctrl-c", CopyHexSelection, None),
            KeyBinding::new("ctrl-=", HexZoomIn, None),
            KeyBinding::new("ctrl-+", HexZoomIn, None),
            KeyBinding::new("ctrl--", HexZoomOut, None),
            KeyBinding::new("ctrl-0", HexZoomReset, None),
        ]);

        // Load prefs once up front so we can honour
        // `restore_window_bounds` at open time. Loaded again inside
        // the `on_window_should_close` handler to avoid persisting a
        // stale snapshot if the user toggled the pref mid-session.
        let prefs_at_open = Prefs::load();
        // Seed the accent from prefs before the first paint: our own
        // palette global, plus the embedded gpui-component theme's
        // primary so its widgets (radio dots, focus rings) match.
        // Must run after `Theme::change` above (which resets colors).
        theme::set_accent_hex(prefs_at_open.accent_color);
        apply_accent_to_component_theme(cx);

        // Boot-time update check — fire-and-forget, detection only.
        // Respects the Settings → Updates opt-out. The blocking HTTP
        // call runs on the background pool; once it resolves we set
        // the UpdateState global and refresh all windows so the
        // Settings sidebar dot paints on the next frame.
        cx.set_global(updater::UpdateState::default());
        if !prefs_at_open.disable_update_check {
            cx.spawn(async move |cx_async| {
                let result = cx_async
                    .background_executor()
                    .spawn(async move { updater::check_for_update(env!("CARGO_PKG_VERSION")) })
                    .await;
                if let Ok(Some(available)) = result {
                    cx_async.update(|cx| {
                        cx.set_global(updater::UpdateState {
                            available: Some(available),
                        });
                        cx.refresh_windows();
                    });
                }
            })
            .detach();
        }
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
    Otp,
    Hex,
    Settings,
    // I²C-mode panes (shown when `bus == Bus::I2c`).
    I2cScan,
    I2cRead,
    I2cWrite,
    I2cVerify,
    I2cErase,
    I2cBlank,
}

/// Which bus the GUI is driving. The sidebar toggle flips this and
/// swaps the workflow: SPI shows Detect/Read/Erase/Write/Verify plus
/// the SPI-only diagnostics; I²C shows Scan/Read/Write/Verify/Erase/
/// Blank-check. The CH341 opens in the matching mode per-op.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Bus {
    #[default]
    Spi,
    I2c,
}

impl Pane {
    /// The pane a bus lands on when the toggle switches to it.
    pub fn default_for(bus: Bus) -> Pane {
        match bus {
            Bus::Spi => Pane::Detect,
            Bus::I2c => Pane::I2cScan,
        }
    }
}

/// What the Detect pane caches between renders. Captures both the
/// chip-identification result (raw JEDEC + resolved chip + which
/// source provided the chip) and the source-of-truth marker so the
/// pane can render "DB hit" vs "SFDP fallback" vs "unknown"
/// consistently without re-querying.
#[derive(Clone, Debug)]
pub struct DetectInfo {
    pub jedec: String,
    pub chip: Option<crate::chipdb::Chip>,
    pub source: ChipSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipSource {
    /// JEDEC matched a `chips.toml` entry.
    Database,
    /// JEDEC missed; chip parameters synthesised from SFDP.
    Sfdp,
    /// JEDEC missed and chip carries no SFDP either.
    Unknown,
    /// MISO floats high (no chip) or stuck low (board contention).
    NoChip,
}

/// Intermediate type used only inside `refresh_detect`'s closure
/// (we can't directly construct the outer `Connection` enum from
/// inside the `and_then` because of borrow scopes on the chip's
/// name string).
enum ConnState {
    Ready { name: String, size_kb: u32 },
    NoChip,
}

/// Best-effort 256-byte SFDP read + parse. Swallows SPI errors as
/// `None` (the chip might not implement SFDP and return weird data,
/// or the read could fail on a flaky bus); the Detect pane treats
/// `None` as "no SFDP info to show" rather than surfacing every
/// underlying USB hiccup.
fn read_sfdp_best_effort(ch: &mut Ch341) -> Option<crate::sfdp::Sfdp> {
    let data = crate::spi::read_sfdp(ch, 0, 256).ok()?;
    let parsed = crate::sfdp::parse(&data);
    if parsed.header.valid {
        Some(parsed)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub enum Connection {
    Disconnected,
    NoChip,
    Ready { chip_name: String, size_kb: u32 },
}

#[derive(Clone, Debug)]
pub struct LogLine {
    /// Unix epoch seconds at the time `push_log` was called. Stored
    /// as raw UTC so the renderer can recompute the displayed
    /// `HH:MM:SS` in whichever timezone `prefs.timestamp_local`
    /// currently selects — including for log lines added before
    /// the user flipped the toggle.
    pub timestamp_secs: u64,
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
    /// SPI vs I²C — flipped by the sidebar bus toggle. Swaps which
    /// workflow the sidebar shows and which mode the CH341 opens in.
    pub bus: Bus,
    /// 7-bit addresses that ACKed on the last I²C scan. `None` before
    /// the first scan in this session.
    pub i2c_scan_results: Option<Vec<u8>>,
    /// Last op outcome (either bus) — `(ok, message)` — rendered as a
    /// colored line in the active op pane: green ✓ on success, red ✗ on
    /// failure. Cleared on navigation (`disarm_all`) so it stays scoped
    /// to the pane that produced it, not just buried in the log.
    pub op_result: Option<(bool, String)>,
    pub connection: Connection,
    /// Last-read SR1/SR2/SR3 bytes from the Status pane's "Read"
    /// button. `None` before the first read or after the user
    /// reset state. Held as the raw struct so the pane render
    /// shares the same decoded view (`StatusRegisters::wip` etc.)
    /// the CLI's `ops::status` uses.
    pub status_regs: Option<crate::spi::StatusRegisters>,
    /// Security (OTP) registers from the most recent OTP-pane read.
    /// `None` before the user clicks "Read security registers".
    pub otp_regs: Option<Vec<ops::OtpRegister>>,
    /// Target register (1/2/3) for the OTP pane's erase / write
    /// controls. Read always covers all three; only the destructive
    /// ops need a target.
    pub otp_target_register: u8,
    /// File selected for `otp write` via the OTP pane's Browse button.
    pub otp_write_path: Option<std::path::PathBuf>,
    /// Two-stage arm flags for the OTP pane's erase / write buttons,
    /// same pattern as `erase_armed` / `write_armed`.
    pub otp_erase_armed: bool,
    pub otp_write_armed: bool,
    /// Result of the most recent Detect run. `chip` is `Some` when
    /// JEDEC matched a DB entry OR SFDP synthesised one; `None` for
    /// MISO-stuck states or a chip with no SFDP fallback. The Detect
    /// pane renders this as a card.
    pub detect_result: Option<DetectInfo>,
    /// Parsed SFDP table from the most recent Detect run. Shown in
    /// a second card inside the Detect pane when present, regardless
    /// of whether the chip resolved via DB or SFDP fallback.
    pub detect_sfdp: Option<crate::sfdp::Sfdp>,
    /// Last SFDP dump + parsed result from the SFDP pane's "Read"
    /// button. Both the raw bytes (for the hex preview) and the
    /// decoded `Sfdp` are kept; the pane render formats both.
    pub log_lines: Vec<LogLine>,
    /// Persists scroll position across re-renders; required by
    /// `track_scroll(...)` to keep the log from jumping back to the
    /// top whenever a new line is appended.
    pub log_scroll: ScrollHandle,
    /// True while the activity log is detached into its own window.
    /// The inline log panel is dropped (the active pane takes the
    /// full height) in this state; closing the pop-out window flips
    /// it back.
    pub log_popped_out: bool,
    /// Handle to the pop-out window while it's open, so a repeat
    /// pop-out click activates the existing window instead of
    /// spawning a duplicate. `None` when the log is inline.
    pub log_window: Option<WindowHandle<Root>>,
    /// Handle to the chip-database browser window while it's open, so
    /// a repeat "Browse chip database" click activates the existing
    /// window instead of spawning a duplicate. `None` when closed.
    pub chip_db_window: Option<WindowHandle<Root>>,
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
    /// I²C Write / Verify file selections and the I²C Write / Erase arm
    /// flags — the I²C analogues of `write_input_path` / `write_armed`
    /// / `erase_armed`.
    pub i2c_write_path: Option<std::path::PathBuf>,
    pub i2c_verify_path: Option<std::path::PathBuf>,
    pub i2c_write_armed: bool,
    pub i2c_erase_armed: bool,
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
    /// Dropdown state for picking the I²C chip (24Cxx). I²C has no
    /// JEDEC auto-detect, so every I²C read/write/verify/erase/blank
    /// op resolves the chip from this selection. Shared across all the
    /// I²C op panes (one selection, rendered in each).
    pub i2c_chip_select: Entity<SelectState<Vec<SharedString>>>,
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
        // I²C chip dropdown, populated from the bundled 24Cxx DB. No
        // default selection — the op panes show a "pick a chip"
        // placeholder, and the op methods bail with a log line if none
        // is chosen (I²C has no JEDEC auto-detect to fall back on).
        let i2c_chips: Vec<SharedString> = crate::chipdb::I2cChipDb::load_embedded()
            .iter()
            .map(|c| SharedString::from(c.name.clone()))
            .collect();
        let i2c_chip_select = cx.new(|cx| SelectState::new(i2c_chips, None, window, cx));
        Self {
            selected: Pane::Detect,
            bus: Bus::Spi,
            i2c_scan_results: None,
            op_result: None,
            connection: Connection::Disconnected,
            status_regs: None,
            otp_regs: None,
            otp_target_register: 1,
            otp_write_path: None,
            otp_erase_armed: false,
            otp_write_armed: false,
            detect_result: None,
            detect_sfdp: None,
            log_lines: vec![LogLine {
                timestamp_secs: now_unix_secs(),
                text: "etch341 ready. Plug in a CH341A and click Detect chip.".into(),
            }],
            log_scroll: ScrollHandle::new(),
            log_popped_out: false,
            log_window: None,
            chip_db_window: None,
            erase_armed: false,
            write_armed: false,
            write_input_path: None,
            verify_input_path: None,
            i2c_write_path: None,
            i2c_verify_path: None,
            i2c_write_armed: false,
            i2c_erase_armed: false,
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
            i2c_chip_select,
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

    /// Bump the hex view font size by `delta` (typically ±1), clamped
    /// to the supported range and saved to prefs. No-op at the
    /// clamp boundary so we don't churn the file on repeated
    /// keypresses past the limit.
    ///
    /// Re-scrolls to the same row index before returning so the user
    /// stays parked at the same byte offset across zoom steps —
    /// without this the underlying pixel scroll offset maps to a
    /// different row at the new row height and the viewport "jumps."
    /// The row-height arithmetic mirrors `hex_view`'s `+5` formula.
    pub fn nudge_hex_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.prefs.hex_font_size + delta)
            .clamp(crate::prefs::HEX_FONT_MIN, crate::prefs::HEX_FONT_MAX);
        if (next - self.prefs.hex_font_size).abs() < f32::EPSILON {
            return;
        }
        let top = uniform_list_top_index(&self.hex_scroll, self.prefs.hex_font_size + 5.0);
        self.prefs.hex_font_size = next;
        self.hex_scroll.scroll_to_item(top, ScrollStrategy::Top);
        self.persist_font_size(cx);
    }

    /// Same for the strings view. Separate from hex on purpose —
    /// the strings list and hex grid have different density needs.
    /// Strings rows have no explicit height — uniform_list measures
    /// them from natural text content, which is `font_size *
    /// line_height` plus a hair. `font_size + 4` matches what gpui
    /// produces with default line-height for mono fonts in the
    /// 8-24px range.
    pub fn nudge_strings_font(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.prefs.strings_font_size + delta)
            .clamp(crate::prefs::HEX_FONT_MIN, crate::prefs::HEX_FONT_MAX);
        if (next - self.prefs.strings_font_size).abs() < f32::EPSILON {
            return;
        }
        let top = uniform_list_top_index(&self.strings_scroll, self.prefs.strings_font_size + 4.0);
        self.prefs.strings_font_size = next;
        self.strings_scroll.scroll_to_item(top, ScrollStrategy::Top);
        self.persist_font_size(cx);
    }

    /// Keybinding handler: cmd/ctrl + `=` / `+`. Only fires when
    /// the Hex pane is selected; routes to whichever sub-view is
    /// currently visible (hex grid or strings list).
    pub fn hex_zoom_in(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            self.nudge_strings_font(1.0, cx);
        } else {
            self.nudge_hex_font(1.0, cx);
        }
    }

    /// Keybinding handler: cmd/ctrl + `-`.
    pub fn hex_zoom_out(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            self.nudge_strings_font(-1.0, cx);
        } else {
            self.nudge_hex_font(-1.0, cx);
        }
    }

    /// Keybinding handler: cmd/ctrl + `0`. Resets the active sub-
    /// view's font to the original default. Same scroll-preserve
    /// trick as the nudge handlers.
    pub fn hex_zoom_reset(&mut self, cx: &mut Context<Self>) {
        if self.selected != Pane::Hex {
            return;
        }
        if self.hex_show_strings {
            let top =
                uniform_list_top_index(&self.strings_scroll, self.prefs.strings_font_size + 4.0);
            self.prefs.strings_font_size = crate::prefs::HEX_FONT_DEFAULT;
            self.strings_scroll.scroll_to_item(top, ScrollStrategy::Top);
        } else {
            let top = uniform_list_top_index(&self.hex_scroll, self.prefs.hex_font_size + 5.0);
            self.prefs.hex_font_size = crate::prefs::HEX_FONT_DEFAULT;
            self.hex_scroll.scroll_to_item(top, ScrollStrategy::Top);
        }
        self.persist_font_size(cx);
    }

    /// Settings → Appearance accent swatch. Updates our palette
    /// global + the gpui-component theme primary, persists the
    /// choice, and re-renders. The pop-out log window picks it up
    /// for free via its observe subscription on this AppView.
    pub fn set_accent(&mut self, hex: u32, cx: &mut Context<Self>) {
        if self.prefs.accent_color == hex {
            return;
        }
        self.prefs.accent_color = hex;
        theme::set_accent_hex(hex);
        apply_accent_to_component_theme(cx);
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("accent save failed: {e}"));
        }
        cx.notify();
    }

    /// Settings → Updates toggle. `enabled` is the user-facing
    /// "check on launch" switch; we persist its inverse
    /// (`disable_update_check`). Enabling also kicks an immediate
    /// check so the user gets feedback without relaunching.
    pub fn set_update_check_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.prefs.disable_update_check = !enabled;
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("update-check pref save failed: {e}"));
        }
        if enabled {
            self.check_for_updates_now(cx);
        } else {
            cx.notify();
        }
    }

    /// Settings → Updates → "Check now". Re-runs the GitHub check in
    /// the background and updates the global; the sidebar dot +
    /// Updates row repaint when it resolves.
    pub fn check_for_updates_now(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ checking for updates".into());
        cx.notify();
        cx.spawn(async move |_weak, cx_async| {
            let result = cx_async
                .background_executor()
                .spawn(async move { updater::check_for_update(env!("CARGO_PKG_VERSION")) })
                .await;
            cx_async.update(|cx| {
                let available = result.ok().flatten();
                cx.set_global(updater::UpdateState { available });
                cx.refresh_windows();
            });
        })
        .detach();
    }

    /// Open the pending release's GitHub page in the default browser.
    /// No-op (with a log line) if no update is currently pending.
    pub fn open_release_page(&mut self, cx: &mut Context<Self>) {
        let Some(update) = updater::available(cx) else {
            self.push_log("no update pending".into());
            cx.notify();
            return;
        };
        // Defense-in-depth: the URL comes from the GitHub API JSON,
        // and we hand it to the OS opener. There's no shell (it's a
        // `Command` arg, not `sh -c`), so no command injection — but
        // a tampered response (e.g. a defeated-TLS MITM) could swap
        // in a non-web scheme like `file:` / `smb:` that the opener
        // would happily launch. Require https:// before spawning.
        if !update.html_url.starts_with("https://") {
            self.push_log(format!(
                "refusing to open release URL with unexpected scheme: {}",
                update.html_url
            ));
            cx.notify();
            return;
        }
        // Same per-OS launcher as `open_prefs_folder`; all three
        // accept a URL and hand it to the default browser.
        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "windows")]
        let cmd = "explorer";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let cmd: &str = "";
        if cmd.is_empty() {
            self.push_log("open release page: unsupported platform".into());
            cx.notify();
            return;
        }
        match std::process::Command::new(cmd)
            .arg(&update.html_url)
            .spawn()
        {
            Ok(_) => self.push_log(format!("Opened {}", update.html_url)),
            Err(e) => self.push_log(format!("open release page: {e}")),
        }
        cx.notify();
    }

    /// Settings → Log timestamps toggle. Storage stays UTC; this
    /// only flips how existing + new log lines render. Saves
    /// immediately so the next launch lands on the same display.
    pub fn set_timestamp_local(&mut self, local: bool, cx: &mut Context<Self>) {
        if self.prefs.timestamp_local == local {
            return;
        }
        self.prefs.timestamp_local = local;
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("timestamp display save failed: {e}"));
        }
        cx.notify();
    }

    /// Wipe the activity log. Bound to the log pane's clear button.
    /// Not undoable on purpose — the log is a running narrative,
    /// not a document. Cheap to re-fill: just run an op.
    pub fn clear_log(&mut self, cx: &mut Context<Self>) {
        self.log_lines.clear();
        cx.notify();
    }

    /// Detach the activity log into its own window. A second click
    /// while it's already open just activates the existing window.
    /// The pop-out renders the same buffer live (it observes this
    /// AppView), so nothing here moves or copies the log itself —
    /// we only flip the inline panel off and track the window.
    pub fn pop_out_log(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.log_window {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            return;
        }
        self.log_popped_out = true;
        cx.notify();

        let app = cx.entity();
        // Defer so we open the window after the current effect cycle
        // returns the borrowed entities to the app (open_window wants
        // a clean &mut App).
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, gpui::size(px(560.0), px(420.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // Same transparent-titlebar + client-decoration
                    // recipe the main window uses, so the pop-out
                    // wears the app's dark chrome rather than a
                    // jarring native bar. `title` still labels it in
                    // the OS window list / dock.
                    titlebar: Some(TitlebarOptions {
                        title: Some("etch341 — Activity Log".into()),
                        ..TitleBar::title_bar_options()
                    }),
                    app_id: Some("etch341".into()),
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                {
                    let app = app.clone();
                    move |window, cx| {
                        window.set_client_inset(px(10.0));
                        let log_view = cx.new(|cx| log::LogWindow::new(app.clone(), cx));
                        // Re-dock the inline log when the pop-out closes.
                        // `observe_release` fires when the LogWindow entity
                        // tears down — which happens however the window was
                        // closed (close button, Cmd/Ctrl+W, OS). The earlier
                        // `on_window_should_close` approach worked on macOS
                        // but not on X11, where gpui doesn't route the
                        // window-manager close through that callback, so the
                        // inline log never came back (issue #1). Tying it to
                        // entity lifecycle instead is platform-agnostic.
                        let close_app = app.downgrade();
                        cx.observe_release(&log_view, move |_, cx| {
                            let _ = close_app.update(cx, |this, cx| {
                                this.log_popped_out = false;
                                this.log_window = None;
                                cx.notify();
                            });
                        })
                        .detach();
                        cx.new(|cx| Root::new(log_view, window, cx))
                    }
                },
            );
            match opened {
                Ok(handle) => {
                    app.update(cx, |this, _| this.log_window = Some(handle));
                }
                Err(e) => {
                    // Couldn't open — undo the inline-hidden state so
                    // the user isn't left with no log at all.
                    app.update(cx, |this, cx| {
                        this.log_popped_out = false;
                        this.push_log(format!("pop-out log failed: {e}"));
                        cx.notify();
                    });
                }
            }
        });
    }

    /// Detect pane → "Browse chip database" button. Opens the chip
    /// catalogue in its own window (same open_window + observe_release
    /// lifecycle as the pop-out log). The browser is self-contained —
    /// it reads the embedded DB directly, so unlike the log it needs no
    /// handle back to AppView for its data; the only AppView coupling
    /// is clearing `chip_db_window` on close so a re-click reopens
    /// rather than activating a dead handle.
    pub fn open_chip_db(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.chip_db_window {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let app = cx.entity();
        // Defer so the window opens after the current effect cycle
        // hands the borrowed entities back to a clean &mut App.
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, gpui::size(px(720.0), px(560.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("etch341 — Chip Database".into()),
                        ..TitleBar::title_bar_options()
                    }),
                    app_id: Some("etch341".into()),
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                {
                    let app = app.clone();
                    move |window, cx| {
                        window.set_client_inset(px(10.0));
                        let view = cx.new(|cx| chipdb_browser::ChipDbBrowser::new(window, cx));
                        // Clear the handle when the window tears down,
                        // however it was closed (button, Cmd/Ctrl+W, OS).
                        let close_app = app.downgrade();
                        cx.observe_release(&view, move |_, cx| {
                            let _ = close_app.update(cx, |this, cx| {
                                this.chip_db_window = None;
                                cx.notify();
                            });
                        })
                        .detach();
                        cx.new(|cx| Root::new(view, window, cx))
                    }
                },
            );
            match opened {
                Ok(handle) => {
                    app.update(cx, |this, _| this.chip_db_window = Some(handle));
                }
                Err(e) => {
                    app.update(cx, |this, cx| {
                        this.push_log(format!("chip-database window failed: {e}"));
                        cx.notify();
                    });
                }
            }
        });
    }

    /// Sidebar bus-toggle handler. Switches SPI ↔ I²C, lands on that
    /// bus's default pane, and clears any armed destructive trigger so
    /// nothing fires across a mode switch.
    pub fn set_bus(&mut self, bus: Bus, cx: &mut Context<Self>) {
        if self.bus == bus {
            return;
        }
        self.bus = bus;
        self.selected = Pane::default_for(bus);
        self.disarm_all();
        cx.notify();
    }

    /// Reset every armed destructive trigger (SPI + OTP + I²C). Called
    /// on any pane navigation and on a bus switch, so an armed action
    /// can never fire on a stale click after the user moves away.
    pub fn disarm_all(&mut self) {
        self.erase_armed = false;
        self.write_armed = false;
        self.otp_erase_armed = false;
        self.otp_write_armed = false;
        self.i2c_write_armed = false;
        self.i2c_erase_armed = false;
        self.op_result = None;
    }

    /// Record an op outcome (either bus): log it *and* stash it for the
    /// active pane to render as a colored line (green ✓ / red ✗). Pass
    /// `ok = false` for failures so the pane renders it red.
    fn set_op_result(&mut self, ok: bool, text: String) {
        self.push_log(text.clone());
        self.op_result = Some((ok, text));
    }

    /// I²C clock for ops. Standard mode (100 kHz) — the safe default
    /// every 24Cxx supports; a settings-driven I²C speed control is a
    /// follow-up. Never exceeds the 400 kHz the transport enforces.
    fn i2c_speed(&self) -> u32 {
        100
    }

    /// I²C bus scan — probes 0x08..0x77 and records the ACKing
    /// addresses. Mirrors `start_read`'s spawn → background → open →
    /// op → update shape, but opens the CH341 in I²C mode.
    pub fn start_i2c_scan(&mut self, cx: &mut Context<Self>) {
        self.op_result = None;
        self.push_log("→ i2c scan (0x08..0x77)".into());
        cx.notify();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    crate::i2c::scan(&mut ch, 0x08..=0x77).map_err(|e| format!("scan: {e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(hits) => {
                        if hits.is_empty() {
                            this.push_log(
                                "scan: no devices responded — note a blank EEPROM (all 0xFF) \
                                 won't show up; pick its chip and read it directly"
                                    .into(),
                            );
                        } else {
                            let list = hits
                                .iter()
                                .map(|a| format!("0x{a:02X}"))
                                .collect::<Vec<_>>()
                                .join(" ");
                            this.push_log(format!("scan: {} ACK → {list}", hits.len()));
                        }
                        this.i2c_scan_results = Some(hits);
                    }
                    Err(err) => {
                        this.i2c_scan_results = None;
                        this.set_op_result(false, format!("Scan failed: {err}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C read — dumps the selected chip to a timestamped file (same
    /// naming + save dir as the SPI Read pane). Requires a chip
    /// selection; logs a hint and bails if none is picked.
    pub fn start_i2c_read(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_select.read(cx).selected_value().cloned() else {
            self.set_op_result(false, "Pick a chip first — I²C has no auto-detect".into());
            cx.notify();
            return;
        };
        let chip_name = chip_name.to_string();
        let path = read_output_path(&self.prefs);
        self.push_log(format!("→ i2c read ({chip_name}) → {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let path_for_task = path.clone();
        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-read");
                    crate::i2c_ops::read(
                        &mut ch,
                        &chip,
                        0,
                        chip.size_bytes,
                        0,
                        &path_for_task,
                        &mut sink,
                    )
                    .map_err(|e| format!("read: {e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => this.set_op_result(
                        true,
                        format!("Read {size} bytes from {name} → {}", path.display()),
                    ),
                    Err(err) => this.set_op_result(false, format!("Read failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Currently-selected I²C chip name, or `None` if the dropdown is
    /// still on its placeholder.
    fn i2c_chip_name(&self, cx: &mut Context<Self>) -> Option<String> {
        self.i2c_chip_select
            .read(cx)
            .selected_value()
            .map(|v| v.to_string())
    }

    /// File picker for the I²C Write pane (mirrors `pick_write_file`).
    pub fn pick_i2c_write_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("EEPROM dumps", &["bin", "rom", "eep"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for i2c write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.i2c_write_path = Some(path);
                this.i2c_write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// File picker for the I²C Verify pane.
    pub fn pick_i2c_verify_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("EEPROM dumps", &["bin", "rom", "eep"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for i2c verify: {}", path.display()));
                this.i2c_verify_path = Some(path);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Write — two-stage arm/confirm (destructive). Needs both a
    /// file and a chip selection.
    pub fn arm_or_fire_i2c_write(&mut self, cx: &mut Context<Self>) {
        if self.i2c_write_path.is_none() {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        }
        if self.i2c_chip_name(cx).is_none() {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        }
        if self.i2c_write_armed {
            self.i2c_write_armed = false;
            self.start_i2c_write(cx);
        } else {
            self.i2c_write_armed = true;
            self.push_log("⚠ i2c write armed: click again to confirm".into());
            cx.notify();
        }
    }

    fn start_i2c_write(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(chip_name)) = (self.i2c_write_path.clone(), self.i2c_chip_name(cx))
        else {
            self.set_op_result(false, "Need a chip and a file".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c write ({chip_name}) ← {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    if data.len() as u32 > chip.size_bytes {
                        return Err(format!(
                            "file is {} bytes but {} only holds {}",
                            data.len(),
                            chip.name,
                            chip.size_bytes
                        ));
                    }
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut wsink = GuiSink::new(progress.clone(), "i2c-wr");
                    crate::i2c_ops::write(&mut ch, &chip, 0, &data, 0, &mut wsink)
                        .map_err(|e| format!("write: {e}"))?;
                    // Verify-after-write, matching the SPI Write pane.
                    let mut vsink = GuiSink::new(progress, "i2c-vfy");
                    let mismatches =
                        crate::i2c_ops::verify(&mut ch, &chip, &data, 0, 0, &mut vsink)
                            .map_err(|e| format!("verify: {e}"))?;
                    Ok::<_, String>((chip.name, data.len(), mismatches))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, n, 0)) => {
                        this.set_op_result(true, format!("Wrote {n} bytes to {name} (verified)"))
                    }
                    Ok((name, n, m)) => this.set_op_result(
                        false,
                        format!("Wrote {n} bytes to {name} but verify found {m} mismatch(es)"),
                    ),
                    Err(err) => this.set_op_result(false, format!("Write failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Verify — read-only, no confirmation. Needs a file + chip.
    pub fn start_i2c_verify(&mut self, cx: &mut Context<Self>) {
        let (Some(path), Some(chip_name)) = (self.i2c_verify_path.clone(), self.i2c_chip_name(cx))
        else {
            self.set_op_result(false, "Need a chip and a file".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c verify ({chip_name}) vs {}", path.display()));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-vfy");
                    crate::i2c_ops::verify(&mut ch, &chip, &data, 0, 0, &mut sink)
                        .map_err(|e| format!("verify: {e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(0) => this.set_op_result(true, "Chip matches the file".into()),
                    Ok(m) => this.set_op_result(false, format!("{m} byte(s) differ")),
                    Err(err) => this.set_op_result(false, format!("Verify failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Erase — two-stage arm/confirm (destructive: writes 0xFF
    /// over the whole chip).
    pub fn arm_or_fire_i2c_erase(&mut self, cx: &mut Context<Self>) {
        if self.i2c_chip_name(cx).is_none() {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        }
        if self.i2c_erase_armed {
            self.i2c_erase_armed = false;
            self.start_i2c_erase(cx);
        } else {
            self.i2c_erase_armed = true;
            self.push_log("⚠ i2c erase armed: click again to confirm".into());
            cx.notify();
        }
    }

    fn start_i2c_erase(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_name(cx) else {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        };
        self.push_log(format!(
            "→ i2c erase ({chip_name}) — writing 0xFF everywhere"
        ));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-erase");
                    crate::i2c_ops::erase(&mut ch, &chip, 0, &mut sink)
                        .map_err(|e| format!("erase: {e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => {
                        this.set_op_result(true, format!("Erased {name} to 0xFF ({size} bytes)"))
                    }
                    Err(err) => this.set_op_result(false, format!("Erase failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// I²C Blank check — confirm every byte reads 0xFF. Read-only.
    pub fn start_i2c_blank_check(&mut self, cx: &mut Context<Self>) {
        let Some(chip_name) = self.i2c_chip_name(cx) else {
            self.set_op_result(false, "Pick a chip first".into());
            cx.notify();
            return;
        };
        self.push_log(format!("→ i2c blank check ({chip_name})"));
        cx.notify();
        self.spawn_progress_poller(cx);

        let progress = self.progress.clone();
        let speed = self.i2c_speed();
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let chip = crate::i2c_ops::resolve_chip(&chip_name)
                        .map_err(|e| format!("chip: {e}"))?;
                    let mut ch = Ch341::open_i2c(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    let mut sink = GuiSink::new(progress, "i2c-blank");
                    crate::i2c_ops::blank_check(&mut ch, &chip, 0, &mut sink)
                        .map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((chip.name, chip.size_bytes))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((name, size)) => this
                        .set_op_result(true, format!("{name} is blank — all 0xFF ({size} bytes)")),
                    Err(err) => this.set_op_result(false, format!("Blank check: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Settings → Hex viewer → "Reset to defaults" button. Resets
    /// *both* font sizes — Cmd+0 only resets the active sub-view,
    /// but a settings-level reset is the natural "put it all back"
    /// affordance. No scroll-preserve since the user is on the
    /// Settings pane, not Hex.
    pub fn reset_hex_fonts(&mut self, cx: &mut Context<Self>) {
        self.prefs.hex_font_size = crate::prefs::HEX_FONT_DEFAULT;
        self.prefs.strings_font_size = crate::prefs::HEX_FONT_DEFAULT;
        self.persist_font_size(cx);
    }

    /// Shared save-and-redraw path for font-size changes. Failures
    /// only surface in the activity log (silent on success — these
    /// happen on every keystroke during a zoom session).
    fn persist_font_size(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.prefs.save() {
            self.push_log(format!("font size save failed: {e}"));
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
            self.set_op_result(false, "Pick an input file first".into());
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
            self.set_op_result(false, "Pick an input file first".into());
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
                        this.set_op_result(true, format!("Wrote {n} bytes to {name} (verified)"))
                    }
                    Err(err) => this.set_op_result(false, format!("Write failed: {err}")),
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
            self.set_op_result(false, "Pick an input file first".into());
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
                        this.set_op_result(true, format!("All {n} bytes match {name}"))
                    }
                    Ok((name, n, mis)) => this.set_op_result(
                        false,
                        format!("{mis} of {n} bytes differ ({name})"),
                    ),
                    Err(err) => this.set_op_result(false, format!("Verify failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Run `ops::run_detect` synchronously on the UI thread, also
    /// read the chip's SFDP table, and fold both into the session
    /// header / activity log / Detect pane state. USB enumeration,
    /// JEDEC, and SFDP together total roughly 60 ms in practice,
    /// which is acceptable on the UI thread for this command (long
    /// ops use background tasks). Stashing the parsed SFDP into
    /// `self.detect_sfdp` lets the Detect pane render the rich
    /// JESD216 view without a separate "Read SFDP" button.
    pub fn refresh_detect(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ detect".to_string());
        self.op_result = None;
        // `outcome` carries the resolved chip-info plus the SFDP
        // parse (if any). MISO-stuck states short-circuit before
        // the SFDP read so we don't waste time decoding 256 bytes
        // of `0xFF` on a disconnected bus.
        let outcome: Result<(DetectInfo, Option<crate::sfdp::Sfdp>, ConnState), _> =
            Ch341::open(false).and_then(|mut ch| {
                let result = ops::run_detect(&mut ch)?;
                let jedec = result.jedec_string();
                let (chip, source, conn, sfdp) = match result.diagnosis {
                    Diagnosis::Known(c) => {
                        // Even for in-DB chips, read SFDP so the
                        // pane can show the rich table.
                        let sfdp = read_sfdp_best_effort(&mut ch);
                        let conn = ConnState::Ready {
                            name: c.name.clone(),
                            size_kb: c.size_kb,
                        };
                        (Some(c), ChipSource::Database, conn, sfdp)
                    }
                    Diagnosis::UnknownChip => {
                        // Try SFDP as fallback. If it provides a
                        // BFPT, synthesise a chip; either way keep
                        // the parsed SFDP for the pane.
                        let synth = ops::synthesize_from_sfdp(&mut ch, &jedec)?;
                        let sfdp = read_sfdp_best_effort(&mut ch);
                        match synth {
                            Some(c) => {
                                let conn = ConnState::Ready {
                                    name: c.name.clone(),
                                    size_kb: c.size_kb,
                                };
                                (Some(c), ChipSource::Sfdp, conn, sfdp)
                            }
                            None => (None, ChipSource::Unknown, ConnState::NoChip, sfdp),
                        }
                    }
                    Diagnosis::MisoStuckLow | Diagnosis::MisoFloatsHigh => {
                        (None, ChipSource::NoChip, ConnState::NoChip, None)
                    }
                };
                let info = DetectInfo {
                    jedec,
                    chip,
                    source,
                };
                Ok((info, sfdp, conn))
            });
        match outcome {
            Ok((info, sfdp, conn)) => {
                self.push_log(format!("JEDEC 0x{}", info.jedec));
                match (&info.source, info.chip.as_ref()) {
                    (ChipSource::Database, Some(c)) => {
                        self.push_log(format!("Detected {} ({} KB)", c.name, c.size_kb));
                    }
                    (ChipSource::Sfdp, Some(c)) => {
                        self.push_log(format!(
                            "Detected {} ({} KB, parameters from SFDP)",
                            c.name, c.size_kb
                        ));
                    }
                    (ChipSource::Unknown, _) => {
                        self.push_log(format!(
                            "Unknown JEDEC 0x{}: chip has no SFDP either; add to chips.toml or pass --chip",
                            info.jedec
                        ));
                    }
                    (ChipSource::NoChip, _) => {
                        // Surfaced via run_detect's diagnosis path
                        // — synthesize the right log line from the
                        // jedec ID, which encodes which condition
                        // we're in (000000 vs FFFFFF).
                        if info.jedec == "000000" {
                            self.push_log(
                                "MISO stuck low: target board contention (lift chip or pin 8)"
                                    .into(),
                            );
                        } else {
                            self.push_log(
                                "MISO floats high: no chip detected (check clip, VCC, pin 1)"
                                    .into(),
                            );
                        }
                    }
                    _ => {}
                }
                self.connection = match conn {
                    ConnState::Ready { name, size_kb } => Connection::Ready {
                        chip_name: name,
                        size_kb,
                    },
                    ConnState::NoChip => Connection::NoChip,
                };
                self.detect_result = Some(info);
                self.detect_sfdp = sfdp;
            }
            Err(err) => {
                self.set_op_result(false, format!("Detect failed: {err}"));
                self.connection = Connection::Disconnected;
                self.detect_result = None;
                self.detect_sfdp = None;
            }
        }
        cx.notify();
    }

    fn push_log(&mut self, text: String) {
        self.log_lines.push(LogLine {
            timestamp_secs: now_unix_secs(),
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
                    Ok((name, size)) => this.set_op_result(
                        true,
                        format!("Read {size} bytes from {name} → {}", path.display()),
                    ),
                    Err(err) => this.set_op_result(false, format!("Read failed: {err}")),
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
                        this.set_op_result(true, format!("Erased {name} — chip is now blank"))
                    }
                    Err(err) => this.set_op_result(false, format!("Erase failed: {err}")),
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
        self.op_result = None;
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
                    Err(err) => this.set_op_result(false, format!("Status read failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read the three security (OTP) registers in the background and
    /// stash them in `self.otp_regs` for the OTP pane. Mirrors
    /// `start_read_status` — same JEDEC-first guard, no progress bar
    /// (three 256-byte reads finish well under the poll interval).
    pub fn start_read_otp(&mut self, cx: &mut Context<Self>) {
        self.push_log("→ security registers".into());
        self.op_result = None;
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
                    ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;

            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        let blank = regs.iter().filter(|r| r.is_blank()).count();
                        this.push_log(format!(
                            "Security registers OK: {} read, {blank} blank (0xFF)",
                            regs.len()
                        ));
                        this.otp_regs = Some(regs);
                    }
                    Err(err) => {
                        this.set_op_result(false, format!("Security registers read failed: {err}"))
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Set the target register (1/2/3) for the OTP erase / write
    /// controls. Re-disarms both since the target changed under them.
    pub fn set_otp_target_register(&mut self, register: u8, cx: &mut Context<Self>) {
        self.otp_target_register = register;
        self.otp_erase_armed = false;
        self.otp_write_armed = false;
        cx.notify();
    }

    /// File picker for the OTP write source. Reuses the Write pane's
    /// last-directory memory so the two share a starting folder.
    pub fn pick_otp_file(&mut self, cx: &mut Context<Self>) {
        let start_dir = self.prefs.last_write_dir.clone();
        cx.spawn(async move |weak, cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .add_filter("Binary", &["bin", "rom"])
                .add_filter("All files", &["*"]);
            if let Some(dir) = start_dir {
                dialog = dialog.set_directory(dir);
            }
            let Some(handle) = dialog.pick_file().await else {
                return;
            };
            let path = handle.path().to_path_buf();
            weak.update(cx, |this, cx| {
                this.push_log(format!("Picked for OTP write: {}", path.display()));
                if let Some(parent) = path.parent() {
                    this.prefs.last_write_dir = Some(parent.to_path_buf());
                    let _ = this.prefs.save();
                }
                this.otp_write_path = Some(path);
                this.otp_write_armed = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-stage erase trigger for the OTP pane. First click arms,
    /// second fires.
    pub fn arm_or_fire_otp_erase(&mut self, cx: &mut Context<Self>) {
        if self.otp_erase_armed {
            self.otp_erase_armed = false;
            self.start_otp_erase(cx);
        } else {
            self.otp_erase_armed = true;
            self.push_log(format!(
                "⚠ OTP erase armed (register {}): click again to confirm",
                self.otp_target_register
            ));
            cx.notify();
        }
    }

    fn start_otp_erase(&mut self, cx: &mut Context<Self>) {
        let register = self.otp_target_register;
        self.push_log(format!("→ erase security register {register}"));
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    ops::ensure_chip_present(&mut ch).map_err(|e| format!("{e}"))?;
                    ops::otp_erase(&mut ch, register).map_err(|e| format!("{e}"))?;
                    // Re-read so the result card reflects the erase.
                    ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok(regs) => {
                        this.otp_regs = Some(regs);
                        this.set_op_result(
                            true,
                            format!("Security register {register} erased to 0xFF"),
                        );
                    }
                    Err(err) => this.set_op_result(false, format!("OTP erase failed: {err}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Two-stage write trigger for the OTP pane. Writes from offset 0
    /// of the selected register; use the CLI `--start` for a partial
    /// write. Requires a file to be picked first.
    pub fn arm_or_fire_otp_write(&mut self, cx: &mut Context<Self>) {
        if self.otp_write_path.is_none() {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        }
        if self.otp_write_armed {
            self.otp_write_armed = false;
            self.start_otp_write(cx);
        } else {
            self.otp_write_armed = true;
            self.push_log(format!(
                "⚠ OTP write armed (register {}): click again to confirm",
                self.otp_target_register
            ));
            cx.notify();
        }
    }

    fn start_otp_write(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.otp_write_path.clone() else {
            self.set_op_result(false, "Pick an input file first".into());
            cx.notify();
            return;
        };
        let register = self.otp_target_register;
        self.push_log(format!(
            "→ write security register {register} from {}",
            path.display()
        ));
        cx.notify();
        let speed = self.prefs.spi_speed_khz;
        cx.spawn(async move |weak, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let data = std::fs::read(&path).map_err(|e| format!("read input: {e}"))?;
                    let mut ch = Ch341::open(false).map_err(|e| format!("open: {e}"))?;
                    ch.set_clock(speed).map_err(|e| format!("set clock: {e}"))?;
                    ops::ensure_chip_present(&mut ch).map_err(|e| format!("{e}"))?;
                    ops::otp_write(&mut ch, register, 0, &data).map_err(|e| format!("{e}"))?;
                    let len = data.len();
                    let regs = ops::read_otp_registers(&mut ch).map_err(|e| format!("{e}"))?;
                    Ok::<_, String>((len, regs))
                })
                .await;
            weak.update(cx, |this, cx| {
                match outcome {
                    Ok((len, regs)) => {
                        this.otp_regs = Some(regs);
                        this.set_op_result(
                            true,
                            format!(
                                "Security register {register} written ({len} byte(s), verified)"
                            ),
                        );
                    }
                    Err(err) => this.set_op_result(false, format!("OTP write failed: {err}")),
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
                    Ok((name, size)) => this.set_op_result(
                        true,
                        format!("{name} is blank — all {size} bytes are 0xFF"),
                    ),
                    Err(err) => this.set_op_result(false, format!("Blank check failed: {err}")),
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
/// `$HOME` so the dump persists past reboots. The local-time
/// `YYYY-MM-DD_HH-MM-SS` suffix makes consecutive reads land in
/// distinct, human-readable files that still sort chronologically.
/// Hyphens (not colons) in the time so the name is legal on Windows
/// and tidy in Finder.
fn read_output_path(prefs: &Prefs) -> std::path::PathBuf {
    let dir = prefs.read_output_dir.clone().unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
    });
    // Local wall-clock; fall back to UTC if the offset can't be
    // determined (the `time` crate's documented thread edge case).
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let stamp = format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    );
    dir.join(format!("etch341-read-{stamp}.bin"))
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
            .on_action(cx.listener(|this: &mut AppView, _: &HexZoomIn, _, cx| {
                this.hex_zoom_in(cx);
            }))
            .on_action(cx.listener(|this: &mut AppView, _: &HexZoomOut, _, cx| {
                this.hex_zoom_out(cx);
            }))
            .on_action(cx.listener(|this: &mut AppView, _: &HexZoomReset, _, cx| {
                this.hex_zoom_reset(cx);
            }))
            .child(TitleBar::new())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(sidebar::render(self.selected, self.bus, cx))
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
                                    otp_regs: self.otp_regs.as_deref(),
                                    otp_target_register: self.otp_target_register,
                                    otp_write_path: self.otp_write_path.as_deref(),
                                    otp_erase_armed: self.otp_erase_armed,
                                    otp_write_armed: self.otp_write_armed,
                                    read_output_dir: self.prefs.read_output_dir.as_deref(),
                                    detect_result: self.detect_result.as_ref(),
                                    detect_sfdp: self.detect_sfdp.as_ref(),
                                    hex_font_size: self.prefs.hex_font_size,
                                    strings_font_size: self.prefs.strings_font_size,
                                    timestamp_local: self.prefs.timestamp_local,
                                    update_check_enabled: !self.prefs.disable_update_check,
                                    i2c_scan_results: self.i2c_scan_results.as_deref(),
                                    i2c_chip_select: &self.i2c_chip_select,
                                    i2c_write_path: self.i2c_write_path.as_deref(),
                                    i2c_verify_path: self.i2c_verify_path.as_deref(),
                                    i2c_write_armed: self.i2c_write_armed,
                                    i2c_erase_armed: self.i2c_erase_armed,
                                    op_result: self.op_result.as_ref(),
                                };
                                let outer = div().flex_1().min_h(px(0.0)).min_w(px(0.0));
                                // Settings has no op activity worth a log
                                // panel; a popped-out log lives in its own
                                // window. Either way the active pane takes
                                // the full height (no split).
                                if self.selected == Pane::Settings || self.log_popped_out {
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
                                                let _ = weak.update(cx, |this, cx| {
                                                    if this.prefs.log_panel_height != Some(new_h) {
                                                        this.prefs.log_panel_height = Some(new_h);
                                                        let _ = this.prefs.save();
                                                    }
                                                    // Keep the log pinned to the newest
                                                    // line as the pane grows / shrinks.
                                                    // Resizing changes the viewport height
                                                    // without appending a line, so the
                                                    // paint-time scroll-to-bottom flag set
                                                    // by push_log is stale and the bottom
                                                    // drifts out of view (issue #2). Re-
                                                    // request it on every resize tick.
                                                    this.log_scroll.scroll_to_bottom();
                                                    cx.notify();
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
                                                        self.prefs.timestamp_local,
                                                        &self.log_scroll,
                                                        cx,
                                                    )),
                                            ),
                                    )
                                }
                            }),
                    ),
            )
    }
}

/// Current wall-clock as Unix epoch seconds. Storage form for log
/// entries so the renderer can format in whichever timezone the user
/// has selected without losing precision on past entries.
fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Render a Unix epoch second as `HH:MM:SS`, either in UTC or the
/// system's local time depending on `local`. Falls back to UTC if
/// the local offset can't be determined (e.g. mid-thread-spawn on
/// some platforms — the `time` crate's documented edge case).
pub fn format_log_time(secs: u64, local: bool) -> String {
    use time::{OffsetDateTime, UtcOffset};
    let utc =
        OffsetDateTime::from_unix_timestamp(secs as i64).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let dt = if local {
        let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
        utc.to_offset(offset)
    } else {
        utc
    };
    format!("{:02}:{:02}:{:02}", dt.hour(), dt.minute(), dt.second())
}
