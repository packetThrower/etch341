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

use crate::ops::{self, Diagnosis, ProgressSink};
use crate::prefs::Prefs;
use crate::programmer::Programmer;

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

mod app;
mod bios_diff;
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
    Bios,
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

/// Chip read-back + sorted differing offsets from the last failed SPI
/// verify, kept so the Verify pane's "View diff in Hex" button can drop
/// the actual chip bytes into the Hex pane with the mismatches
/// pre-highlighted and Cmd+G-navigable.
pub struct VerifyDiff {
    pub file_bytes: Arc<Vec<u8>>,
    pub chip_bytes: Arc<Vec<u8>>,
    pub offsets: Vec<usize>,
}

/// One row of the side-by-side verify-diff list. Re-exported from the
/// shared [`crate::diff`] core so the GUI list and the CLI `diff` /
/// `verify --diff` renderers group mismatches through the exact same
/// logic.
pub use crate::diff::DiffRow;

/// Which column of the side-by-side diff a selection covers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    File,
    Chip,
}

/// Active side-by-side diff view showing only the differing runs plus a
/// couple of context lines each side. Built either by `show_verify_diff`
/// (file vs chip read-back) or `show_file_diff` (two picked files);
/// `None` = not in the diff view (the Hex pane shows its normal viewer).
pub struct DiffView {
    /// Left side (red / removed). Named `file` for the verify case but
    /// holds whichever buffer is on the left.
    pub file: Arc<Vec<u8>>,
    /// Right side (green / added).
    pub chip: Arc<Vec<u8>>,
    /// Flat display rows (headers + data rows) for the virtualized list.
    pub rows: Arc<Vec<DiffRow>>,
    /// Header-row index of each region, for Prev/Next nav.
    pub region_rows: Vec<usize>,
    /// Currently-focused region (index into `region_rows`).
    pub current: usize,
    /// Total differing bytes (the summary count).
    pub total_diffs: usize,
    /// Pane heading — "Verify diff" or "Compare files".
    pub title: String,
    /// Short name for the left/red side (e.g. "file" or "old.bin").
    pub left_label: String,
    /// Short name for the right/green side (e.g. "chip" or "new.bin").
    pub right_label: String,
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
fn read_sfdp_best_effort(ch: &mut Programmer) -> Option<crate::sfdp::Sfdp> {
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
    /// Handle to the BIOS settings-diff window, replaced on each new
    /// comparison and cleared on close.
    pub bios_diff_window: Option<WindowHandle<Root>>,
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
    /// Chip read-back + differing offsets from the last failed verify;
    /// drives the Verify pane's "View diff in Hex" button. `None` when
    /// the last verify matched, errored, or hasn't run.
    pub verify_diff: Option<VerifyDiff>,
    /// Active side-by-side diff view (file vs chip). Set by the Verify
    /// pane's "View diff" button, cleared by the diff view's Close.
    /// When `Some`, the Hex pane renders the diff instead of its viewer.
    pub hex_diff: Option<DiffView>,
    /// Byte selection in the diff view: which column, plus the
    /// (anchor, cursor) offsets. Drives Cmd/Ctrl+C copy from either side.
    pub diff_selection: Option<(DiffSide, usize, usize)>,
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
    /// Scroll handle for the verify-diff list (its own, so it doesn't
    /// fight the hex viewer's scroll position).
    pub diff_scroll: UniformListScrollHandle,
    /// Line index to highlight in the hex view (set by
    /// `jump_to_hex_offset`). Sticky — stays until the next jump.
    pub hex_highlight_line: Option<usize>,
    /// Same idea but for the strings list. Separate handle so the
    /// two views can scroll independently.
    pub strings_scroll: UniformListScrollHandle,
    /// Toggle between raw hex dump (false) and extracted-strings view (true).
    pub hex_show_strings: bool,
    /// Whether the byte-colour legend (the "?" dropdown next to the Hex
    /// pane's footer) is expanded.
    pub hex_show_legend: bool,
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
    /// BIOS Setup explorer: the loaded image path, the resolved
    /// settings (parsed once at load), the list scroll handle, and the
    /// label filter synced from `bios_search_state`.
    pub bios_input_path: Option<std::path::PathBuf>,
    pub bios_settings: Option<Arc<Vec<crate::uefi::Setting>>>,
    /// Menu tree (form → sub-forms) for the drill-down navigator.
    pub bios_tree: Option<Arc<Vec<crate::uefi::FormNode>>>,
    /// Decoded boot order, shown via the navigator's "Boot order" entry.
    pub bios_boot: Option<Arc<Vec<crate::uefi::BootEntry>>>,
    /// Firmware identity (vendor / project / platform) for the header line.
    pub bios_id: Option<crate::uefi::BiosId>,
    /// Selected form in the navigator; `None` shows every setting.
    pub bios_selected_form: Option<String>,
    /// When true, the settings list shows only changed-from-default rows.
    pub bios_changed_only: bool,
    pub bios_scroll: UniformListScrollHandle,
    pub bios_nav_scroll: UniformListScrollHandle,
    pub bios_search_term: String,
    pub bios_search_state: Entity<InputState>,
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
        // BIOS-explorer label filter — same Input→String bridge as the
        // hex Find field. Only the Change event matters here (no jump).
        let bios_search_state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter settings by label (e.g. VT-d, SATA)")
        });
        let bios_sub = cx.subscribe_in(
            &bios_search_state,
            window,
            |this: &mut AppView, state, event: &InputEvent, _, cx| {
                if let InputEvent::Change = event {
                    this.bios_search_term = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );

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
            bios_diff_window: None,
            erase_armed: false,
            write_armed: false,
            write_input_path: None,
            verify_input_path: None,
            verify_diff: None,
            hex_diff: None,
            diff_selection: None,
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
            diff_scroll: UniformListScrollHandle::new(),
            strings_scroll: UniformListScrollHandle::new(),
            hex_highlight_line: None,
            hex_show_strings: false,
            hex_show_legend: false,
            hex_selection: None,
            hex_selecting: false,
            hex_search_term: String::new(),
            hex_search_state,
            bios_input_path: None,
            bios_settings: None,
            bios_tree: None,
            bios_boot: None,
            bios_id: None,
            bios_selected_form: None,
            bios_changed_only: false,
            bios_scroll: UniformListScrollHandle::new(),
            bios_nav_scroll: UniformListScrollHandle::new(),
            bios_search_term: String::new(),
            bios_search_state,
            i2c_chip_select,
            _subscriptions: vec![sub, bios_sub],
            progress: Arc::new(SharedProgress::default()),
            prefs: Prefs::load(),
            focus_handle: cx.focus_handle(),
        }
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
                        // `on_window_should_close` fires *synchronously* on
                        // the close request, on every backend the current
                        // gpui supports — including both Linux ones: the X11
                        // backend runs the handler on WM_DELETE_WINDOW and the
                        // Wayland backend on xdg_toplevel `Close`, destroying
                        // the window when the handler returns `true`.
                        //
                        // This replaced an `observe_release` approach that
                        // tied the re-dock to the LogWindow entity being
                        // dropped. That teardown is synchronous on
                        // macOS/Windows but *deferred to a later event-loop
                        // turn on Wayland*, so on an idle app the inline log
                        // never came back (issue #1, Wayland — closing the
                        // pop-out left the main window with no log at all).
                        // Hooking the close request directly sidesteps the
                        // entity-drop timing entirely.
                        let close_app = app.downgrade();
                        window.on_window_should_close(cx, move |_window, cx| {
                            let _ = close_app.update(cx, |this, cx| {
                                this.log_popped_out = false;
                                this.log_window = None;
                                cx.notify();
                            });
                            true
                        });
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
                        let view = cx.new(|cx| {
                            chipdb_browser::ChipDbBrowser::new(app.downgrade(), window, cx)
                        });
                        // Clear the handle when the window closes, so a
                        // re-click reopens instead of activating a dead
                        // handle. Uses `on_window_should_close` for the same
                        // reason the pop-out log does (see `pop_out_log`):
                        // `observe_release`'s entity teardown is deferred on
                        // Wayland, which would strand the stale handle and
                        // block reopening the browser.
                        let close_app = app.downgrade();
                        window.on_window_should_close(cx, move |_window, cx| {
                            let _ = close_app.update(cx, |this, cx| {
                                this.chip_db_window = None;
                                cx.notify();
                            });
                            true
                        });
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

    /// Open (or replace) the BIOS settings-diff window with a computed
    /// comparison. Each compare replaces the previous window so the
    /// content is never stale. Modelled on `open_chip_db`.
    pub fn open_bios_diff(
        &mut self,
        diffs: Vec<crate::uefi::SettingDiff>,
        a_name: String,
        b_name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(handle) = self.bios_diff_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        let app = cx.entity();
        let diffs = std::sync::Arc::new(diffs);
        cx.defer(move |cx| {
            let bounds = Bounds::centered(None, gpui::size(px(1000.0), px(700.0)), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("etch341 — BIOS Setting Diff".into()),
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
                        let view = cx.new(|cx| {
                            bios_diff::BiosDiffView::new(
                                app.downgrade(),
                                diffs.clone(),
                                a_name.clone(),
                                b_name.clone(),
                                window,
                                cx,
                            )
                        });
                        let close_app = app.downgrade();
                        window.on_window_should_close(cx, move |_window, cx| {
                            let _ = close_app.update(cx, |this, cx| {
                                this.bios_diff_window = None;
                                cx.notify();
                            });
                            true
                        });
                        cx.new(|cx| Root::new(view, window, cx))
                    }
                },
            );
            match opened {
                Ok(handle) => {
                    app.update(cx, |this, _| this.bios_diff_window = Some(handle));
                }
                Err(e) => {
                    app.update(cx, |this, cx| {
                        this.push_log(format!("BIOS diff window failed: {e}"));
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
        // Drop any stored verify diff so the other bus's Verify pane
        // doesn't surface a stale "View diff in Hex" button.
        self.verify_diff = None;
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
                    if this.hex_diff.is_some() {
                        this.diff_step_region(true, cx);
                    } else {
                        this.find_next(cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this: &mut AppView, _: &FindPrevAction, _, cx| {
                    if this.hex_diff.is_some() {
                        this.diff_step_region(false, cx);
                    } else {
                        this.find_prev(cx);
                    }
                }),
            )
            .on_action(
                cx.listener(|this: &mut AppView, _: &CopyHexSelection, _, cx| {
                    if this.hex_diff.is_some() {
                        this.copy_diff_selection(cx);
                    } else {
                        this.copy_hex_selection(cx);
                    }
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
                                    verify_has_diff: self.verify_diff.is_some(),
                                    hex_path: self.hex_input_path.as_deref(),
                                    hex_bytes: self.hex_bytes.clone(),
                                    hex_strings: self.hex_strings.clone(),
                                    hex_byte_matches: self.hex_byte_matches.clone(),
                                    hex_match_total: self.hex_match_starts.len(),
                                    hex_current_match: self.hex_current_match,
                                    hex_scroll: self.hex_scroll.clone(),
                                    strings_scroll: self.strings_scroll.clone(),
                                    hex_diff: self.hex_diff.as_ref(),
                                    diff_scroll: self.diff_scroll.clone(),
                                    diff_selection: self.diff_selection,
                                    hex_highlight_line: self.hex_highlight_line,
                                    hex_show_strings: self.hex_show_strings,
                                    hex_show_legend: self.hex_show_legend,
                                    hex_selection: self.selection_range(),
                                    hex_search_term: self.hex_search_term.as_str(),
                                    hex_search_state: &self.hex_search_state,
                                    bios_path: self.bios_input_path.as_deref(),
                                    bios_settings: self.bios_settings.clone(),
                                    bios_tree: self.bios_tree.clone(),
                                    bios_boot: self.bios_boot.clone(),
                                    bios_id: self.bios_id.as_ref(),
                                    bios_selected_form: self.bios_selected_form.as_deref(),
                                    bios_changed_only: self.bios_changed_only,
                                    bios_scroll: self.bios_scroll.clone(),
                                    bios_nav_scroll: self.bios_nav_scroll.clone(),
                                    bios_search_term: self.bios_search_term.as_str(),
                                    bios_search_state: &self.bios_search_state,
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
