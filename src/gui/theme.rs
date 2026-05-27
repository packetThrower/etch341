//! Color palette + cross-platform font choices for etch341. Dark
//! theme only.

use gpui::{Hsla, Rgba};

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
/// - Linux:   `monospace` (freedesktop generic; resolves via
///   fontconfig to the user's preferred monospace, usually DejaVu
///   Sans Mono on Debian/Ubuntu/Fedora)
///
/// Without this, `font_family("Menlo")` on Windows fell back to a
/// thin substitute that rendered the Hex pane bytes faintly /
/// "ghostly" against the dark background — the user-visible bug
/// this constant fixes.
pub const MONO_FONT: &str = if cfg!(target_os = "macos") {
    "Menlo"
} else if cfg!(target_os = "windows") {
    "Consolas"
} else {
    "monospace"
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

/// Single accent. Used sparingly — one blue per visible region.
pub fn accent_blue() -> Hsla {
    from_rgb(0x0A84FF, 1.0)
}
pub fn accent_blue_hover() -> Hsla {
    from_rgb(0x409CFF, 1.0)
}
pub fn accent_blue_tint() -> Hsla {
    from_rgb(0x007AFF, 0.25)
}

/// Background tint for selected hex bytes. Neutral (white-ish) so
/// it reads as "selected" rather than "matched" — search matches
/// already use accent_blue_tint, and the two need to be visually
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

pub fn text_primary() -> Hsla {
    from_rgb(0xFFFFFF, 0.95)
}
pub fn text_secondary() -> Hsla {
    from_rgb(0xFFFFFF, 0.65)
}
pub fn text_tertiary() -> Hsla {
    from_rgb(0xFFFFFF, 0.40)
}
