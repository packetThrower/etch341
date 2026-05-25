//! User preferences. Lives at `~/.config/etch341/prefs.toml`; the
//! file is optional — missing or malformed contents yield defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// SPI clock speed in kHz. The CH341A's I²C-stream set-speed
    /// command supports 20, 100, 400, 750; higher rates exist but
    /// require vendor commands etch341 doesn't yet implement.
    /// Default 750 = the highest of the supported set.
    pub spi_speed_khz: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            spi_speed_khz: 750,
        }
    }
}

impl Prefs {
    pub fn path() -> Option<PathBuf> {
        std::env::var("HOME").ok().map(|h| {
            PathBuf::from(h)
                .join(".config")
                .join("etch341")
                .join("prefs.toml")
        })
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
        let body = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, body)
    }
}
