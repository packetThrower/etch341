//! User preferences. Lives at `~/.config/etch341/prefs.toml`; the
//! file is optional — missing or malformed contents yield defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// SPI clock speed in kHz.
    pub spi_speed_khz: u32,
    /// Directory of the last file picked via the Hex pane's Browse
    /// button. Used to start the next pick in the same place.
    pub last_hex_dir: Option<PathBuf>,
    /// Same idea for the Write pane.
    pub last_write_dir: Option<PathBuf>,
    /// Same idea for the Verify pane.
    pub last_verify_dir: Option<PathBuf>,
    /// Pixel height of the activity-log resizable panel. Saved on
    /// drag, restored on launch.
    pub log_panel_height: Option<f32>,
    /// Opt-in: when true, the window's last bounds are saved on
    /// close and restored on next launch. Off by default — pinning
    /// the window state changes a behavior most fresh-install users
    /// don't expect, so we require an explicit toggle in Settings.
    pub restore_window_bounds: bool,
    /// Last-known window geometry. Only honoured when
    /// `restore_window_bounds` is true. `None` until the first save.
    pub window_bounds: Option<WindowGeometry>,
    /// Directory the Read pane writes its `etch341-read-<unix>.bin`
    /// dump into. `None` falls back to `$HOME` (the original
    /// behaviour). Set via Settings → Read save location.
    pub read_output_dir: Option<PathBuf>,
    /// Font size (in px) for the Hex pane's hex+ASCII view.
    /// Adjustable on the fly with Cmd/Ctrl + / - / 0 while the Hex
    /// pane is showing the hex view, or via the Settings pane.
    pub hex_font_size: f32,
    /// Font size (in px) for the Hex pane's strings view. Same
    /// keybindings as `hex_font_size`, but only one of the two is
    /// adjusted at a time (whichever view is currently visible).
    pub strings_font_size: f32,
    /// Render activity-log timestamps in the user's local time
    /// zone instead of UTC. Storage is always raw UTC seconds —
    /// this only affects how existing and new log lines are
    /// displayed. Off by default since UTC matches the historical
    /// behaviour and is what shows up in any log files we'd add
    /// later.
    pub timestamp_local: bool,
    /// Accent color as a `0xRRGGBB` value, chosen from the Settings
    /// → Appearance swatches. Drives both etch341's own accent and
    /// the embedded gpui-component widgets' primary color.
    pub accent_color: u32,
}

/// On-disk snapshot of the window's position + size. Stored as
/// plain `f32`s so the `Bounds<Pixels>` <-> toml conversion is
/// trivial — `gpui::Pixels` doesn't `Serialize` directly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Default font size (in px) for the Hex pane's hex view and
/// strings view. Mirrors the historical hardcoded `text_size(px(11.0))`
/// so an existing prefs.toml without these fields lands on the
/// same visual default.
pub const HEX_FONT_DEFAULT: f32 = 11.0;
/// Allowed range for the hex / strings view font size, clamped on
/// every adjustment. 8 keeps glyphs legible on hi-DPI screens; 24
/// is well past the "I need to lean back from the screen" point.
pub const HEX_FONT_MIN: f32 = 8.0;
pub const HEX_FONT_MAX: f32 = 24.0;

impl Default for Prefs {
    fn default() -> Self {
        Self {
            spi_speed_khz: 750,
            last_hex_dir: None,
            last_write_dir: None,
            last_verify_dir: None,
            log_panel_height: None,
            restore_window_bounds: false,
            window_bounds: None,
            read_output_dir: None,
            hex_font_size: HEX_FONT_DEFAULT,
            strings_font_size: HEX_FONT_DEFAULT,
            timestamp_local: false,
            // Apple "blue"; mirrors gui::theme::DEFAULT_ACCENT_HEX
            // (kept in sync by hand — prefs is the non-GUI layer and
            // can't reach the feature-gated theme module).
            accent_color: 0x0A84FF,
        }
    }
}

impl Prefs {
    /// Per-OS prefs file location:
    ///   - Linux:   `$HOME/.config/etch341/prefs.toml` (XDG-ish)
    ///   - macOS:   `$HOME/.config/etch341/prefs.toml` (same; many
    ///     cross-platform CLIs follow XDG on macOS too, and the
    ///     existing dotfile would orphan if we moved to
    ///     `~/Library/Application Support`)
    ///   - Windows: `%APPDATA%\etch341\prefs.toml`
    ///
    /// Returns `None` if the relevant env var isn't set (rare —
    /// system without `$HOME` on Unix or without `APPDATA` on
    /// Windows). All callers tolerate `None` (no save, no restore,
    /// no "Open folder" button in Settings).
    pub fn path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA")
                .ok()
                .map(|a| PathBuf::from(a).join("etch341").join("prefs.toml"))
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME").ok().map(|h| {
                PathBuf::from(h)
                    .join(".config")
                    .join("etch341")
                    .join("prefs.toml")
            })
        }
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body =
            toml::to_string_pretty(self).map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, body)
    }
}
