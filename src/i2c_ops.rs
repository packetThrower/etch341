//! High-level I²C operations: scan / read / write / verify / blank_check.
//!
//! Each op takes a `&mut dyn I2cTransport` so it can be unit-tested
//! against a mock. The 24Cxx wire-level details live in `i2c::`.

use crate::chipdb::{I2cChip, I2cChipDb};
use crate::error::{Error, Result};
use crate::i2c::{self, I2cTransport};
use crate::ops::ProgressSink;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::Path;

const READ_CHUNK_BYTES: u32 = 256;

/// Resolve an I²C chip from the embedded DB by name. (No JEDEC
/// equivalent on I²C, so chip selection is always explicit.)
pub fn resolve_chip(name: &str) -> Result<I2cChip> {
    I2cChipDb::load_embedded()
        .find_by_name(name)
        .cloned()
        .ok_or_else(|| Error::ChipNotRecognized(name.to_string()))
}

/// Probe the standard EEPROM range (0x50..=0x57) and any extras the
/// caller asks for. Returns each 7-bit address that ACKed.
pub fn scan(bus: &mut dyn I2cTransport) -> Result<Vec<u8>> {
    i2c::scan(bus, 0x08..=0x77)
}

/// Read `len` bytes from `mem_addr` into `output`. Emits SHA-256 +
/// summary to stdout for parity with the SPI `read` op.
pub fn read(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    mem_addr: u32,
    len: u32,
    pin_straps: u8,
    output: &Path,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let end = mem_addr.saturating_add(len);
    if end > chip.size_bytes {
        return Err(Error::AddressOutOfRange {
            addr: mem_addr,
            len,
            chip_size: chip.size_bytes,
        });
    }
    let mut out = File::create(output)?;
    let mut hasher = Sha256::new();
    progress.start(len as u64);

    let mut addr = mem_addr;
    while addr < end {
        let n = std::cmp::min(READ_CHUNK_BYTES, end - addr);
        let data = i2c::read(bus, chip, addr, n, pin_straps)?;
        out.write_all(&data)?;
        hasher.update(&data);
        addr += n;
        progress.update((addr - mem_addr) as u64);
    }
    progress.finish();
    println!("Read OK  : {} bytes → {}", len, output.display());
    println!("SHA-256  : {}", hex::encode(hasher.finalize()));
    Ok(())
}

/// Read a `len`-byte region into memory — the in-memory sibling of
/// [`read`] (which streams to a file). Used by the GUI's I²C verify-diff
/// view, which needs the EEPROM's actual bytes alongside the differing
/// offsets.
pub fn read_bytes(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    mem_addr: u32,
    len: u32,
    pin_straps: u8,
    progress: &mut dyn ProgressSink,
) -> Result<Vec<u8>> {
    let end = mem_addr.saturating_add(len);
    if end > chip.size_bytes {
        return Err(Error::AddressOutOfRange {
            addr: mem_addr,
            len,
            chip_size: chip.size_bytes,
        });
    }
    progress.start(len as u64);
    let mut buf = Vec::with_capacity(len as usize);
    let mut addr = mem_addr;
    while addr < end {
        let n = std::cmp::min(READ_CHUNK_BYTES, end - addr);
        let data = i2c::read(bus, chip, addr, n, pin_straps)?;
        buf.extend_from_slice(&data);
        addr += n;
        progress.update((addr - mem_addr) as u64);
    }
    progress.finish();
    Ok(buf)
}

/// Write `data` starting at `mem_addr`. Splits at the smaller of the
/// EEPROM page boundary and the CH341 packet limit; ACK-polls
/// between chunks to wait out the chip's internal write cycle.
pub fn write(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    mem_addr: u32,
    data: &[u8],
    pin_straps: u8,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    progress.start(data.len() as u64);
    i2c::write(bus, chip, mem_addr, data, pin_straps)?;
    progress.update(data.len() as u64);
    progress.finish();
    Ok(())
}

/// Read back the region covered by `data` and count mismatches.
/// Returns 0 on a clean verify; nonzero counts indicate the chip
/// and `data` differ.
pub fn verify(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    expected: &[u8],
    mem_addr: u32,
    pin_straps: u8,
    progress: &mut dyn ProgressSink,
) -> Result<u32> {
    let len = expected.len() as u32;
    let end = mem_addr.saturating_add(len);
    if end > chip.size_bytes {
        return Err(Error::AddressOutOfRange {
            addr: mem_addr,
            len,
            chip_size: chip.size_bytes,
        });
    }
    progress.start(len as u64);
    let mut mismatches = 0u32;
    let mut addr = mem_addr;
    let mut off = 0usize;
    while addr < end {
        let n = std::cmp::min(READ_CHUNK_BYTES, end - addr) as usize;
        let got = i2c::read(bus, chip, addr, n as u32, pin_straps)?;
        for (i, (g, e)) in got.iter().zip(&expected[off..off + n]).enumerate() {
            if g != e {
                mismatches += 1;
                if mismatches <= 5 {
                    eprintln!(
                        "  mismatch at 0x{:08X}: expected 0x{:02X}, got 0x{:02X}",
                        addr + i as u32,
                        e,
                        g
                    );
                }
            }
        }
        addr += n as u32;
        off += n;
        progress.update((addr - mem_addr) as u64);
    }
    progress.finish();
    Ok(mismatches)
}

/// Confirm every byte on the chip is 0xFF (the canonical erased
/// state for EEPROMs that have been written to with all-ones).
/// First non-0xFF byte returns `Error::NotBlank`.
pub fn blank_check(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    pin_straps: u8,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    progress.start(chip.size_bytes as u64);
    let mut addr = 0u32;
    while addr < chip.size_bytes {
        let n = std::cmp::min(READ_CHUNK_BYTES, chip.size_bytes - addr);
        let data = i2c::read(bus, chip, addr, n, pin_straps)?;
        for (i, b) in data.iter().enumerate() {
            if *b != 0xFF {
                return Err(Error::NotBlank {
                    addr: addr + i as u32,
                    value: *b,
                });
            }
        }
        addr += n;
        progress.update(addr as u64);
    }
    progress.finish();
    Ok(())
}

/// "Erase" an I²C EEPROM by writing 0xFF to every byte. EEPROMs
/// don't have a sector-erase concept like NOR flash; this is just
/// a full-chip write of 0xFF.
pub fn erase(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    pin_straps: u8,
    progress: &mut dyn ProgressSink,
) -> Result<()> {
    let buf = vec![0xFFu8; chip.size_bytes as usize];
    write(bus, chip, 0, &buf, pin_straps, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i2c::test_support::{MockI2c, Step};

    fn chip_24c02() -> I2cChip {
        I2cChip {
            name: "24C02".into(),
            size_bytes: 256,
            page_size: 8,
            addr_width: 1,
            slave_addr_bits: 0,
            write_cycle_ms: 5,
        }
    }

    #[test]
    fn verify_returns_zero_on_match() {
        let chip = chip_24c02();
        let expected = vec![0xAA; 10];
        let mut mock = MockI2c::new([Step {
            slave: 0x50,
            tx: vec![0x00],
            rx_len: 10,
            read_back: vec![0xAA; 10],
            nack: false,
        }]);
        let n = verify(&mut mock, &chip, &expected, 0, 0, &mut NullSink).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn verify_counts_mismatches() {
        let chip = chip_24c02();
        let expected = vec![0xAA, 0xBB, 0xCC, 0xDD];
        let mut mock = MockI2c::new([Step {
            slave: 0x50,
            tx: vec![0x00],
            rx_len: 4,
            read_back: vec![0xAA, 0x00, 0xCC, 0x00],
            nack: false,
        }]);
        let n = verify(&mut mock, &chip, &expected, 0, 0, &mut NullSink).unwrap();
        assert_eq!(n, 2);
    }

    /// Build the sequence of mock Steps for reading `total` bytes
    /// starting at `start_addr` from a 24Cxx with 1-byte addressing.
    /// Mirrors the chunking the transport layer actually issues
    /// (MAX_READ_CHUNK = 31 bytes per transaction).
    fn read_steps(start_addr: u32, data: &[u8]) -> Vec<Step> {
        let chunk = crate::i2c::MAX_READ_CHUNK as u32;
        let mut steps = Vec::new();
        let mut addr = start_addr;
        let end = start_addr + data.len() as u32;
        let mut off = 0usize;
        while addr < end {
            let n = std::cmp::min(chunk, end - addr) as usize;
            steps.push(Step {
                slave: 0x50,
                tx: vec![addr as u8],
                rx_len: n,
                read_back: data[off..off + n].to_vec(),
                nack: false,
            });
            addr += n as u32;
            off += n;
        }
        steps
    }

    #[test]
    fn blank_check_passes_when_all_ff() {
        let chip = chip_24c02();
        let data = vec![0xFFu8; 256];
        let mut mock = MockI2c::new(read_steps(0, &data));
        blank_check(&mut mock, &chip, 0, &mut NullSink).unwrap();
    }

    #[test]
    fn blank_check_fails_on_non_ff() {
        let chip = chip_24c02();
        let mut data = vec![0xFFu8; 256];
        data[42] = 0x00;
        let mut mock = MockI2c::new(read_steps(0, &data));
        let err = blank_check(&mut mock, &chip, 0, &mut NullSink).unwrap_err();
        match err {
            Error::NotBlank { addr, value } => {
                assert_eq!(addr, 42);
                assert_eq!(value, 0x00);
            }
            other => panic!("expected NotBlank, got {:?}", other),
        }
    }

    /// Empty ProgressSink for tests. Avoids pulling in real I/O.
    struct NullSink;
    impl ProgressSink for NullSink {}
}
