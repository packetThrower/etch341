//! Chip database loader. Reads `chips/chips.toml` at runtime.

use crate::error::{Error, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Chip {
    pub name: String,
    /// Six hex characters, uppercase, e.g. "EF4018".
    pub jedec_id: String,
    pub size_kb: u32,
    pub page_size: u32,
    pub sector_size: u32,
    pub erase_time_ms: u32,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Deserialize)]
struct ChipFile {
    chip: Vec<Chip>,
}

#[derive(Debug, Clone)]
pub struct ChipDb {
    chips: Vec<Chip>,
}

impl ChipDb {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let parsed: ChipFile = toml::from_str(&raw).map_err(|source| Error::ChipDb {
            path: path.display().to_string(),
            source,
        })?;
        Ok(Self { chips: parsed.chip })
    }

    /// Parse the chip DB baked into the binary at build time.
    /// Avoids a runtime filesystem lookup for installed binaries.
    pub fn load_embedded() -> Self {
        const TOML: &str = include_str!("../chips/chips.toml");
        let parsed: ChipFile = toml::from_str(TOML)
            .expect("embedded chips/chips.toml failed to parse (build-time bug)");
        Self { chips: parsed.chip }
    }

    pub fn find_by_jedec(&self, jedec_id: &str) -> Option<&Chip> {
        let needle = jedec_id.to_ascii_uppercase();
        self.chips
            .iter()
            .find(|c| c.jedec_id.to_ascii_uppercase() == needle)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Chip> {
        self.chips
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Chip> {
        self.chips.iter()
    }

    pub fn len(&self) -> usize {
        self.chips.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty()
    }
}

/// I²C serial-EEPROM entry. Separate struct from SPI `Chip` because
/// the relevant attributes barely overlap: no JEDEC ID, no sector
/// erase, page size is much smaller, and addressing can require
/// stuffing bits into the slave address byte (24C04/08/16).
#[derive(Debug, Clone, Deserialize)]
pub struct I2cChip {
    pub name: String,
    pub size_bytes: u32,
    pub page_size: u32,
    /// 1 for ≤ 256-byte chips (single memory-address byte on the wire),
    /// 2 for ≥ 4-KB chips (two memory-address bytes).
    pub addr_width: u8,
    /// Number of high memory-address bits that get OR'd into the
    /// 7-bit slave address byte instead of riding on the data bus.
    /// 24C04 = 1, 24C08 = 2, 24C16 = 3; everything else = 0.
    pub slave_addr_bits: u8,
    /// Worst-case time the chip stays busy after a page-write,
    /// used as the ACK-polling timeout.
    pub write_cycle_ms: u32,
}

#[derive(Debug, Deserialize)]
struct I2cChipFile {
    chip: Vec<I2cChip>,
}

#[derive(Debug, Clone)]
pub struct I2cChipDb {
    chips: Vec<I2cChip>,
}

impl I2cChipDb {
    pub fn load_embedded() -> Self {
        const TOML: &str = include_str!("../chips/i2c_chips.toml");
        let parsed: I2cChipFile = toml::from_str(TOML)
            .expect("embedded chips/i2c_chips.toml failed to parse (build-time bug)");
        Self { chips: parsed.chip }
    }

    pub fn find_by_name(&self, name: &str) -> Option<&I2cChip> {
        self.chips
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &I2cChip> {
        self.chips.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn db_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("chips/chips.toml")
    }

    #[test]
    fn parses_bundled_db() {
        let db = ChipDb::load(&db_path()).expect("parse chips.toml");
        assert!(!db.is_empty(), "chip DB should not be empty");
    }

    #[test]
    fn jedec_ids_are_unique() {
        let db = ChipDb::load(&db_path()).unwrap();
        let mut ids: Vec<String> = db.iter().map(|c| c.jedec_id.to_ascii_uppercase()).collect();
        ids.sort();
        let dup = ids.windows(2).find(|w| w[0] == w[1]);
        assert!(dup.is_none(), "duplicate JEDEC ID: {:?}", dup);
    }

    #[test]
    fn jedec_ids_are_six_hex_chars() {
        let db = ChipDb::load(&db_path()).unwrap();
        for c in db.iter() {
            assert_eq!(
                c.jedec_id.len(),
                6,
                "{}: JEDEC ID must be 6 hex chars, got {:?}",
                c.name,
                c.jedec_id
            );
            assert!(
                c.jedec_id.chars().all(|ch| ch.is_ascii_hexdigit()),
                "{}: JEDEC ID must be hex, got {:?}",
                c.name,
                c.jedec_id
            );
        }
    }

    #[test]
    fn lookup_by_jedec_is_case_insensitive() {
        let db = ChipDb::load(&db_path()).unwrap();
        let first = db.iter().next().unwrap();
        assert!(
            db.find_by_jedec(&first.jedec_id.to_ascii_lowercase())
                .is_some()
        );
        assert!(
            db.find_by_jedec(&first.jedec_id.to_ascii_uppercase())
                .is_some()
        );
    }
}
