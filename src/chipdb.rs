//! Chip database loader.
//!
//! Both DBs (`chips/chips.toml` for SPI NOR + `chips/i2c_chips.toml`
//! for 24Cxx EEPROMs) are baked into the `etch341` binary at build
//! time via `include_str!`, so the shipped binary doesn't touch the
//! filesystem for chip lookup — no external file to install or
//! `~/.config/etch341/chips.toml` to maintain. Editing the bundled
//! catalogue requires a rebuild.
//!
//! The `load_embedded()` constructors below feed the compiled-in
//! strings to `toml::from_str`; the `load(&Path)` constructor is
//! used only by the in-tree unit tests, which round-trip the
//! source-tree files via `CARGO_MANIFEST_DIR`.

use crate::error::{Error, Result};
use serde::Deserialize;
use std::path::Path;

// `erase_time_ms` is parsed from the TOML for completeness (it's
// documented in CLAUDE.md as part of the chip-DB schema) but the
// runtime doesn't read it — chip-erase uses a fixed multi-minute
// timeout instead. Suppress the field-unused lint without dropping
// the schema field.
#[allow(dead_code)]
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

impl Chip {
    /// Operating-voltage class, derived from the JEDEC manufacturer +
    /// memory-type byte rather than stored as a field. Every part in
    /// the bundled DB follows its vendor's family convention, so the
    /// voltage is fully recoverable from the JEDEC id — deriving it
    /// means it can never drift out of sync with that id (storing it
    /// separately would let one be edited without the other).
    ///
    /// 1.8V families: Winbond W25Q*JW (0xEF60), Macronix MX25U
    /// (0xC225), GigaDevice GD25LQ (0xC860), Adesto AT25SL (0x1F42).
    /// Adesto AT25DN512C (0x1F65) is the lone 2.3V part. Everything
    /// else in the catalogue is 3.3V. Used by the GUI chip browser and
    /// the CLI `chips` table; `voltage_matches_notes_where_present`
    /// cross-checks this against the hand-written notes.
    pub fn voltage(&self) -> &'static str {
        let id = self.jedec_id.to_ascii_uppercase();
        match (id.get(0..2).unwrap_or(""), id.get(2..4).unwrap_or("")) {
            ("EF", "60") => "1.8V", // Winbond W25Q*JW
            ("C2", "25") => "1.8V", // Macronix MX25U
            ("C8", "60") => "1.8V", // GigaDevice GD25LQ
            ("1F", "42") => "1.8V", // Adesto AT25SL
            ("1F", "65") => "2.3V", // Adesto AT25DN512C (2.3–3.6V)
            _ => "3.3V",
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChipFile {
    chip: Vec<Chip>,
}

#[derive(Debug, Clone)]
pub struct ChipDb {
    chips: Vec<Chip>,
}

// `load`, `len`, `is_empty` are part of the chipdb's public surface;
// `load_embedded` is what the running binary uses, but the
// file-path-driven variant + size accessors stay available for
// future tooling (a `chips reload` CLI command, a settings-pane
// reload button, integration tests against a fixture TOML).
#[allow(dead_code)]
impl ChipDb {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let parsed: ChipFile = toml::from_str(&raw).map_err(|source| Error::ChipDb {
            path: path.display().to_string(),
            source,
        })?;
        validate_chips(&parsed.chip).map_err(Error::ChipDbInvalid)?;
        Ok(Self { chips: parsed.chip })
    }

    /// Parse the chip DB baked into the binary at build time.
    /// Avoids a runtime filesystem lookup for installed binaries.
    pub fn load_embedded() -> Self {
        const TOML: &str = include_str!("../chips/chips.toml");
        let parsed: ChipFile = toml::from_str(TOML)
            .expect("embedded chips/chips.toml failed to parse (build-time bug)");
        validate_chips(&parsed.chip)
            .unwrap_or_else(|e| panic!("embedded chips/chips.toml invalid (build-time bug): {e}"));
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

impl I2cChip {
    /// Operating voltage. The 24Cxx family is wide-range — variants
    /// span roughly 1.8–5.5V, and the parts are commonly run at 3.3V or
    /// 5V on the CH341A (its 3.3V/5V jumper). The bundled entries are
    /// generic (no variant suffix), so this reports the family range
    /// rather than a single rail. Unlike SPI NOR — per-part and
    /// decodable from the JEDEC id (see `Chip::voltage`) — an I²C
    /// EEPROM carries no manufacturer/voltage id on the wire, so
    /// there's nothing to derive; the range is the honest answer.
    pub fn voltage(&self) -> &'static str {
        "1.8–5.5V"
    }
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
        validate_i2c_chips(&parsed.chip).unwrap_or_else(|e| {
            panic!("embedded chips/i2c_chips.toml invalid (build-time bug): {e}")
        });
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

/// Reject SPI chip entries that would later panic or divide-by-zero
/// at the op layer: `page_size` / `sector_size` are denominators in
/// the size arithmetic (`ops::print_chip_facts`) and a zero
/// `size_kb` is meaningless. The bundled DB is validated at
/// `load_embedded` (a bad edit fails CI), and any future
/// file-loaded DB fails with a typed error rather than panicking
/// deep in an operation.
fn validate_chips(chips: &[Chip]) -> std::result::Result<(), String> {
    for c in chips {
        if c.size_kb == 0 {
            return Err(format!("{}: size_kb must be non-zero", c.name));
        }
        if c.page_size == 0 {
            return Err(format!("{}: page_size must be non-zero", c.name));
        }
        if c.sector_size == 0 {
            return Err(format!("{}: sector_size must be non-zero", c.name));
        }
    }
    Ok(())
}

/// Reject I²C entries with an `addr_width` the protocol layer can't
/// encode (`i2c::addr_bytes` only handles 1 or 2 and panics
/// otherwise) or a zero size / page.
fn validate_i2c_chips(chips: &[I2cChip]) -> std::result::Result<(), String> {
    for c in chips {
        if !matches!(c.addr_width, 1 | 2) {
            return Err(format!(
                "{}: addr_width must be 1 or 2, got {}",
                c.name, c.addr_width
            ));
        }
        if c.size_bytes == 0 {
            return Err(format!("{}: size_bytes must be non-zero", c.name));
        }
        if c.page_size == 0 {
            return Err(format!("{}: page_size must be non-zero", c.name));
        }
    }
    Ok(())
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
    fn voltage_matches_notes_where_present() {
        let db = ChipDb::load(&db_path()).unwrap();
        for c in db.iter() {
            let v = c.voltage();
            assert!(
                matches!(v, "3.3V" | "1.8V" | "2.3V"),
                "{}: unexpected voltage {v}",
                c.name
            );
            // Where a note states a voltage, the derived value must
            // agree — this pins the JEDEC→voltage family map against
            // the hand-written catalogue for every annotated entry.
            for known in ["1.8V", "2.3V", "3.3V"] {
                if c.notes.contains(known) {
                    assert_eq!(v, known, "{}: note says {known}, derived {v}", c.name);
                }
            }
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

    /// The bundled DBs must satisfy the runtime invariants — this is
    /// the CI gate that turns a bad catalogue edit into a failing
    /// test instead of a panic / divide-by-zero in an op. Both
    /// `load_embedded` calls panic on a bad entry, so reaching the
    /// asserts means they validated.
    #[test]
    fn bundled_dbs_pass_validation() {
        assert!(!ChipDb::load_embedded().is_empty());
        assert!(I2cChipDb::load_embedded().iter().next().is_some());
    }

    #[test]
    fn rejects_zero_page_or_sector() {
        let mut chips = vec![Chip {
            name: "BAD".into(),
            jedec_id: "AABBCC".into(),
            size_kb: 1024,
            page_size: 0,
            sector_size: 4096,
            erase_time_ms: 0,
            notes: String::new(),
        }];
        assert!(
            validate_chips(&chips).is_err(),
            "zero page_size must reject"
        );
        chips[0].page_size = 256;
        chips[0].sector_size = 0;
        assert!(
            validate_chips(&chips).is_err(),
            "zero sector_size must reject"
        );
    }

    #[test]
    fn rejects_bad_i2c_addr_width() {
        let chips = vec![I2cChip {
            name: "BAD".into(),
            size_bytes: 256,
            page_size: 8,
            addr_width: 3,
            slave_addr_bits: 0,
            write_cycle_ms: 5,
        }];
        assert!(validate_i2c_chips(&chips).is_err());
    }
}
