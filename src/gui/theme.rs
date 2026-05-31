//! Color palette + cross-platform font choices for etch341. Dark
//! theme only.

use gpui::{Hsla, Rgba};
use std::sync::RwLock;

/// Cross-platform monospace family for hex / address columns / log
/// timestamps / preferences path display / anywhere we want
/// fixed-width text. GPUI's `font_family` takes a single name (no
/// CSS-style fallback chain), so we pick a face that ships
/// pre-installed on each target OS rather than relying on the
/// platform's "monospace" alias:
///
/// - macOS:   `Menlo` (default Terminal.app font since 10.6)
/// - Windows: `Consolas` (shipped with Vista+; Cascadia Mono is the
///   newer Microsoft default but only present on Windows 10 21H2+
///   — Consolas is the safer floor)
/// - Linux:   `DejaVu Sans Mono` (preinstalled on Debian, Ubuntu,
///   Fedora, openSUSE, Arch's `ttf-dejavu` group, and most
///   freedesktop-conforming distros). The freedesktop generic
///   `"monospace"` would also have been correct but gpui's font
///   loader doesn't resolve fontconfig aliases — it wants a real
///   family name, and `"monospace"` fell back to a thin sans-serif
///   that mis-aligned the Hex pane columns the same way Windows
///   did with the Menlo fallback.
///
/// Without this constant, `font_family("Menlo")` on Windows + Linux
/// fell back to a thin substitute that rendered the Hex pane bytes
/// faintly / mis-aligned against the dark background — the
/// user-visible bug this constant fixes.
pub const MONO_FONT: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else if cfg!(target_os = "windows") {
    "Consolas"
} else {
    "DejaVu Sans Mono"
};

fn from_rgb(hex: u32, alpha: f32) -> Hsla {
    Rgba {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: alpha,
    }
    .into()
}

/// Activity-log background. Darkest layer on screen; the viewport
/// the user's eyes are meant to land on first.
pub fn bench_black() -> Hsla {
    from_rgb(0x0B0B0D, 1.0)
}

/// Translucent overlay for sidebars and panels — the "glass" the
/// rest of the chrome floats on.
pub fn workshop_glass() -> Hsla {
    from_rgb(0xFFFFFF, 0.06)
}
pub fn workshop_glass_strong() -> Hsla {
    from_rgb(0xFFFFFF, 0.10)
}

/// Default accent (Apple "blue"). The accent is user-selectable via
/// Settings → Appearance; this is the value a fresh install starts
/// at and the fallback the swatches compare against.
pub const DEFAULT_ACCENT_HEX: u32 = 0x0A84FF;

/// Curated accent presets shown as swatches in Settings. Stored as
/// `(name, 0xRRGGBB, dark_text)`; the name is the swatch tooltip and
/// `dark_text` picks near-black vs near-white button labels.
///
/// The foreground is an explicit per-preset choice rather than pure
/// luma because the two don't agree: saturated red / pink read
/// better with dark text than their (fairly low) luminance predicts,
/// while they're actually *darker* than purple / graphite — so a
/// single luma threshold can't give red/pink dark text without
/// dragging purple/graphite along. `accent_foreground` falls back to
/// luma only for a hypothetical accent not in this list.
pub const ACCENT_PRESETS: &[(&str, u32, bool)] = &[
    ("Blue", 0x0A84FF, false),
    ("Purple", 0xBF5AF2, false),
    ("Pink", 0xFF375F, true),
    ("Red", 0xFF453A, true),
    ("Orange", 0xFF9F0A, true),
    ("Green", 0x30D158, true),
    ("Teal", 0x40CBE0, true),
    ("Graphite", 0x8E8E93, false),
];

/// Current accent, stored as a hex RGB. A process-global rather than
/// threaded through every render call: the palette functions are
/// called all over the render tree and only ever read on the (single)
/// UI thread. `gui::run` seeds it from prefs at startup and
/// `AppView::set_accent` updates it when the user picks a swatch.
static ACCENT_HEX: RwLock<u32> = RwLock::new(DEFAULT_ACCENT_HEX);

pub fn accent_hex() -> u32 {
    *ACCENT_HEX.read().unwrap()
}
pub fn set_accent_hex(hex: u32) {
    *ACCENT_HEX.write().unwrap() = hex;
}

/// Opaque color from a hex RGB — used to paint the preset swatches.
pub fn swatch_color(hex: u32) -> Hsla {
    from_rgb(hex, 1.0)
}

/// Single accent. Used sparingly — one accent per visible region.
/// Reads the user's chosen color; hover / active / tint are derived
/// from it so any accent gets a consistent lighter-on-hover,
/// darker-on-press, and translucent-tint treatment.
pub fn accent() -> Hsla {
    from_rgb(accent_hex(), 1.0)
}
pub fn accent_hover() -> Hsla {
    let mut h = accent();
    h.l = (h.l + 0.12).min(1.0);
    h
}
pub fn accent_active() -> Hsla {
    let mut h = accent();
    h.l = (h.l - 0.08).max(0.0);
    h
}
pub fn accent_tint() -> Hsla {
    let mut h = accent();
    h.a = 0.25;
    h
}

/// Readable text / icon color to lay over the accent: near-black on
/// the presets flagged `dark_text` (orange, teal, green, red, pink),
/// near-white on the rest (blue, purple, graphite). Uses the
/// per-preset flag; for an accent not in `ACCENT_PRESETS` it falls
/// back to a Rec.601 luma threshold.
pub fn accent_foreground() -> Hsla {
    let hex = accent_hex();
    let dark_text = ACCENT_PRESETS
        .iter()
        .find(|(_, h, _)| *h == hex)
        .map(|(_, _, dark)| *dark)
        .unwrap_or_else(|| {
            let r = ((hex >> 16) & 0xFF) as f32;
            let g = ((hex >> 8) & 0xFF) as f32;
            let b = (hex & 0xFF) as f32;
            0.299 * r + 0.587 * g + 0.114 * b > 145.0
        });
    if dark_text {
        from_rgb(0x0B0B0D, 0.92)
    } else {
        from_rgb(0xFFFFFF, 0.95)
    }
}

/// Background tint for selected hex bytes. Neutral (white-ish) so
/// it reads as "selected" rather than "matched" — search matches
/// already use `accent_tint`, and the two need to be visually
/// distinct when a selection covers a match.
pub fn selection_tint() -> Hsla {
    from_rgb(0xFFFFFF, 0.18)
}

pub fn success_green() -> Hsla {
    from_rgb(0x32D74B, 1.0)
}
pub fn caution_red() -> Hsla {
    from_rgb(0xFF453A, 1.0)
}
pub fn warning_amber() -> Hsla {
    from_rgb(0xF5D76E, 1.0)
}

/// Dedicated fixed highlight for the I²C 24Cxx voltage range in the
/// chip browser. Deliberately a constant — NOT `accent()`, which
/// tracks the user's accent setting and so changed the I²C voltage
/// colour (and collided with the 3.3V green) whenever the accent did.
/// A blue keeps it clear of the success/caution/warning traffic light
/// so the wide-range I²C voltage reads as its own category regardless
/// of the chosen accent.
pub fn info_blue() -> Hsla {
    from_rgb(0x5E9EFF, 1.0)
}

/// Standard diff highlight colours for the verify diff view.
/// Deliberately FIXED constants — never `accent()` — so the diff reads
/// like every other diff tool regardless of the user's accent choice.
/// Red marks the file/expected (left) side, green the chip/actual
/// (right) side, following the left=removed / right=added convention.
/// The `_bg` tints back the differing byte cells; the solid colours
/// drive the byte text.
pub fn diff_removed() -> Hsla {
    from_rgb(0xFF6962, 1.0)
}
pub fn diff_removed_bg() -> Hsla {
    from_rgb(0xFF453A, 0.22)
}
pub fn diff_added() -> Hsla {
    from_rgb(0x5BE26B, 1.0)
}
pub fn diff_added_bg() -> Hsla {
    from_rgb(0x32D74B, 0.22)
}

pub fn text_primary() -> Hsla {
    from_rgb(0xFFFFFF, 0.95)
}
pub fn text_secondary() -> Hsla {
    from_rgb(0xFFFFFF, 0.65)
}
pub fn text_tertiary() -> Hsla {
    from_rgb(0xFFFFFF, 0.40)
}
