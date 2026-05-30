//! I²C serial-EEPROM protocol (24Cxx family).
//!
//! The transport (CH341A or a mock) implements [`I2cTransport`]; this
//! module is hardware-agnostic and only knows about I²C bus mechanics
//! and the 24Cxx EEPROM addressing convention.
//!
//! 24Cxx slave address: 7-bit base `0b1010_AAA` where `AAA` is normally
//! the A0/A1/A2 pin straps. On larger 24C04/08/16 the high memory bits
//! are OR'd into the AAA field instead of riding on the bus — see
//! [`compute_slave`].

use crate::chipdb::I2cChip;
use crate::error::{Error, Result};
use std::thread::sleep;
use std::time::Duration;

/// 7-bit base address shared by every 24Cxx EEPROM. Pin straps and
/// memory-high-bit stuffing modify the low three bits.
pub const EEPROM_BASE_ADDR: u8 = 0x50;

/// One I²C transaction. `tx` goes out after a START + slave|W; if
/// `rx_len > 0` a repeated-START switches to slave|R and reads
/// `rx_len` bytes; finally STOP. Errs with [`Error::I2cNack`] if
/// the device fails to ACK the addressed byte.
pub trait I2cTransport {
    fn i2c_transfer(&mut self, slave_7bit: u8, tx: &[u8], rx_len: usize) -> Result<Vec<u8>>;

    /// Address the slave with a 0-byte write transaction and report
    /// whether it ACKed. Used for [`scan`] / detect.
    fn i2c_probe(&mut self, slave_7bit: u8) -> Result<bool>;
}

/// 24Cxx slave-address composition.
///
/// For chips up to 24C16 some of the memory-address high bits live
/// in the slave-address byte (because they don't fit in the single
/// on-bus address byte). `pin_straps` is the A0/A1/A2 pin value the
/// user wired (usually `0` — all pins to ground).
///
/// Returns the 7-bit address the bus will see for a transaction
/// targeting `mem_addr`.
pub fn compute_slave(chip: &I2cChip, mem_addr: u32, pin_straps: u8) -> u8 {
    if chip.slave_addr_bits == 0 {
        // Standard case: pin straps go into the bottom 3 bits.
        return EEPROM_BASE_ADDR | (pin_straps & 0x07);
    }
    // 24C04/08/16: the top `slave_addr_bits` of the memory address
    // override the corresponding pin-strap bits.
    let bits = chip.slave_addr_bits;
    let mask = (1u8 << bits) - 1;
    // Memory-address bit width on the wire is `addr_width * 8`, so
    // the high bits we're stealing are at position `addr_width*8`.
    let high = ((mem_addr >> (chip.addr_width as u32 * 8)) as u8) & mask;
    // Preserve any pin-strap bits above the stolen range.
    let strap_keep = (pin_straps & !mask) & 0x07;
    EEPROM_BASE_ADDR | strap_keep | high
}

/// Encode the on-bus memory address bytes for `chip`. 1-byte chips
/// truncate `mem_addr` to its low 8 bits; 2-byte chips emit big-endian
/// hi/lo. Any bits beyond what fits in `addr_width * 8` belong in the
/// slave-address byte and are added by [`compute_slave`].
pub fn addr_bytes(chip: &I2cChip, mem_addr: u32) -> Vec<u8> {
    match chip.addr_width {
        1 => vec![mem_addr as u8],
        2 => vec![(mem_addr >> 8) as u8, mem_addr as u8],
        other => panic!("unsupported addr_width {other} in chip {:?}", chip.name),
    }
}

/// Read `len` bytes from `mem_addr`. Issues a "dummy write" of the
/// memory address followed by a restart-read of the data, then stops.
/// Splits into ≤ [`MAX_READ_CHUNK`] reads per transaction so each one
/// fits in a single CH341 USB packet.
pub fn read(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    mem_addr: u32,
    len: u32,
    pin_straps: u8,
) -> Result<Vec<u8>> {
    let end = mem_addr.checked_add(len).ok_or(Error::AddressOutOfRange {
        addr: mem_addr,
        len,
        chip_size: chip.size_bytes,
    })?;
    if end > chip.size_bytes {
        return Err(Error::AddressOutOfRange {
            addr: mem_addr,
            len,
            chip_size: chip.size_bytes,
        });
    }
    let mut out = Vec::with_capacity(len as usize);
    let mut addr = mem_addr;
    while addr < end {
        let n = std::cmp::min(MAX_READ_CHUNK as u32, end - addr) as usize;
        let slave = compute_slave(chip, addr, pin_straps);
        let mut chunk = bus.i2c_transfer(slave, &addr_bytes(chip, addr), n)?;
        out.append(&mut chunk);
        addr += n as u32;
    }
    Ok(out)
}

/// Write `data` starting at `mem_addr`. Splits at the smaller of the
/// EEPROM's page boundary or [`MAX_WRITE_DATA_PER_TXN`] so a single
/// page-write can't span two EEPROM pages or overflow the CH341
/// packet. Waits between chunks via ACK-polling.
pub fn write(
    bus: &mut dyn I2cTransport,
    chip: &I2cChip,
    mem_addr: u32,
    data: &[u8],
    pin_straps: u8,
) -> Result<()> {
    let end = mem_addr
        .checked_add(data.len() as u32)
        .ok_or(Error::AddressOutOfRange {
            addr: mem_addr,
            len: data.len() as u32,
            chip_size: chip.size_bytes,
        })?;
    if end > chip.size_bytes {
        return Err(Error::AddressOutOfRange {
            addr: mem_addr,
            len: data.len() as u32,
            chip_size: chip.size_bytes,
        });
    }

    let page = chip.page_size;
    let mut offset = 0usize;
    while offset < data.len() {
        let addr = mem_addr + offset as u32;
        // Bytes left until the next EEPROM page boundary at this address.
        let to_page_end = page - (addr % page);
        let chunk_size = [
            data.len() - offset,
            to_page_end as usize,
            MAX_WRITE_DATA_PER_TXN,
        ]
        .iter()
        .copied()
        .min()
        .unwrap();
        let chunk = &data[offset..offset + chunk_size];

        // Build "addr_bytes ++ chunk" as the tx payload; slave|W is
        // added by the transport.
        let mut tx = addr_bytes(chip, addr);
        tx.extend_from_slice(chunk);
        let slave = compute_slave(chip, addr, pin_straps);
        bus.i2c_transfer(slave, &tx, 0)?;

        // Wait out the chip's internally-timed write cycle (tWR)
        // before touching the bus again. ACK-polling for "ready"
        // would be faster, but the CH341's stream mode never exposes
        // the I2C ACK bit — a write-address probe returns no status
        // byte at all (see ch341::i2c_probe). The old `wait_ready`
        // therefore read a *data* byte and treated a 0xFF readback as
        // "still busy". After a page write the next byte is frequently
        // 0xFF — and *always* 0xFF on a blank chip — so that poll
        // timed out and the whole write failed with `Timeout`. A fixed
        // worst-case sleep is slower per page but actually correct.
        // `write_cycle_ms` is the datasheet tWR max (5 ms for 24C02);
        // double it for headroom against voltage/temp cycle-stretching
        // — under-waiting restarts the next page mid-cycle and garbles
        // it (the v0.2.0 silicon failure on an AT24C02), so the margin
        // is deliberate.
        sleep(Duration::from_millis(
            (chip.write_cycle_ms as u64).max(1) * 2,
        ));
        offset += chunk_size;
    }
    Ok(())
}

/// Probe every 7-bit address in `range` and return the ones that
/// ACKed. Useful first-step diagnostic — equivalent to `i2cdetect`.
pub fn scan(bus: &mut dyn I2cTransport, range: std::ops::RangeInclusive<u8>) -> Result<Vec<u8>> {
    let mut hits = Vec::new();
    for addr in range {
        if bus.i2c_probe(addr)? {
            hits.push(addr);
        }
    }
    Ok(hits)
}

/// Max bytes the CH341 can read in one I²C transaction. Each byte now
/// gets its own `IN` substream command in the request (one `IN | 1`
/// per byte so the master ACKs each, plus a terminating bare `IN`), so
/// the *outgoing* stream grows by one byte per byte read. The CH341
/// USB packet is 32 bytes; once the ~12-byte envelope (the command
/// header, two STARTs, two OUT headers, both slave-address bytes, the
/// STOP and the END) is subtracted, there's room for 20 IN commands.
/// Callers paginate. The old single `IN | n` form fit a larger count
/// but ACKed the final byte, leaving the read unterminated — the
/// multi-chunk corruption this replaces.
pub const MAX_READ_CHUNK: usize = 20;

/// Max data bytes (excluding address) per I²C write transaction. The
/// CH341 USB packet is 32 bytes; once you subtract the envelope, the
/// START/OUT/STOP/END substreams, the slave-address byte, and a
/// worst-case 2-byte memory address, what's left is 23 — round down
/// to 16 for some safety margin and to align nicely with common page
/// sizes (24C04/08/16 all use 16-byte pages).
pub const MAX_WRITE_DATA_PER_TXN: usize = 16;

#[cfg(test)]
pub(crate) mod test_support {
    use super::{I2cTransport, Result};
    use std::collections::VecDeque;

    /// Recorded I²C interaction: slave address, written bytes, and
    /// the bytes the mock should "read back". An empty `read_back`
    /// with `rx_len == 0` represents a pure write.
    #[derive(Debug, Clone)]
    pub struct Step {
        pub slave: u8,
        pub tx: Vec<u8>,
        pub rx_len: usize,
        pub read_back: Vec<u8>,
        /// If true, the transport returns `Error::I2cNack` for this step
        /// — used to test ACK-polling.
        pub nack: bool,
    }

    pub struct MockI2c {
        steps: VecDeque<Step>,
    }

    impl MockI2c {
        pub fn new(steps: impl IntoIterator<Item = Step>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
            }
        }

        pub fn assert_drained(&self) {
            assert!(
                self.steps.is_empty(),
                "{} expected I²C steps were never consumed",
                self.steps.len()
            );
        }
    }

    impl I2cTransport for MockI2c {
        fn i2c_transfer(&mut self, slave_7bit: u8, tx: &[u8], rx_len: usize) -> Result<Vec<u8>> {
            let step = self.steps.pop_front().unwrap_or_else(|| {
                panic!(
                    "unexpected i2c_transfer(slave=0x{:02X}, tx={}, rx_len={})",
                    slave_7bit,
                    hex::encode(tx),
                    rx_len
                )
            });
            assert_eq!(slave_7bit, step.slave, "slave addr mismatch");
            assert_eq!(tx, step.tx.as_slice(), "tx payload mismatch");
            assert_eq!(rx_len, step.rx_len, "rx_len mismatch");
            if step.nack {
                return Err(crate::error::Error::I2cNack { slave_7bit });
            }
            Ok(step.read_back)
        }

        fn i2c_probe(&mut self, _slave_7bit: u8) -> Result<bool> {
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chipdb::I2cChip;
    use test_support::{MockI2c, Step};

    fn chip(name: &str, size: u32, page: u32, aw: u8, sab: u8) -> I2cChip {
        I2cChip {
            name: name.into(),
            size_bytes: size,
            page_size: page,
            addr_width: aw,
            slave_addr_bits: sab,
            write_cycle_ms: 5,
        }
    }

    #[test]
    fn slave_addr_for_24c02_uses_base_plus_pin_straps() {
        let c = chip("24C02", 256, 8, 1, 0);
        assert_eq!(compute_slave(&c, 0x00, 0), 0x50);
        assert_eq!(compute_slave(&c, 0xFF, 0b001), 0x51);
        assert_eq!(compute_slave(&c, 0xFF, 0b111), 0x57);
    }

    #[test]
    fn slave_addr_for_24c16_stuffs_top_3_addr_bits() {
        // 24C16: 2 KB chip, 11-bit memory address. The bottom 8 bits
        // ride the bus; the top 3 bits go into the slave address.
        let c = chip("24C16", 2048, 16, 1, 3);
        // mem 0x000: top bits = 000 → slave 0x50
        assert_eq!(compute_slave(&c, 0x000, 0), 0x50);
        // mem 0x100: top bits = 001 → slave 0x51
        assert_eq!(compute_slave(&c, 0x100, 0), 0x51);
        // mem 0x7FF: top bits = 111 → slave 0x57
        assert_eq!(compute_slave(&c, 0x7FF, 0), 0x57);
    }

    #[test]
    fn addr_bytes_24c02_emits_one_byte() {
        let c = chip("24C02", 256, 8, 1, 0);
        assert_eq!(addr_bytes(&c, 0x42), vec![0x42]);
    }

    #[test]
    fn addr_bytes_24c256_emits_two_bytes_big_endian() {
        let c = chip("24C256", 32768, 64, 2, 0);
        assert_eq!(addr_bytes(&c, 0x1234), vec![0x12, 0x34]);
        assert_eq!(addr_bytes(&c, 0x00FF), vec![0x00, 0xFF]);
    }

    #[test]
    fn read_short_fits_one_transaction() {
        let c = chip("24C02", 256, 8, 1, 0);
        let mut mock = MockI2c::new([Step {
            slave: 0x50,
            tx: vec![0x10],
            rx_len: 4,
            read_back: vec![0xDE, 0xAD, 0xBE, 0xEF],
            nack: false,
        }]);
        assert_eq!(
            read(&mut mock, &c, 0x10, 4, 0).unwrap(),
            vec![0xDE, 0xAD, 0xBE, 0xEF]
        );
        mock.assert_drained();
    }

    #[test]
    fn read_long_splits_into_max_read_chunks() {
        let c = chip("24C256", 32768, 64, 2, 0);
        // 50-byte read should split into 20 + 20 + 10 (MAX_READ_CHUNK).
        let mut mock = MockI2c::new([
            Step {
                slave: 0x50,
                tx: vec![0x00, 0x00],
                rx_len: 20,
                read_back: vec![0xAA; 20],
                nack: false,
            },
            Step {
                slave: 0x50,
                tx: vec![0x00, 0x14],
                rx_len: 20,
                read_back: vec![0xBB; 20],
                nack: false,
            },
            Step {
                slave: 0x50,
                tx: vec![0x00, 0x28],
                rx_len: 10,
                read_back: vec![0xCC; 10],
                nack: false,
            },
        ]);
        let out = read(&mut mock, &c, 0, 50, 0).unwrap();
        assert_eq!(out.len(), 50);
        assert_eq!(&out[..20], &[0xAA; 20]);
        assert_eq!(&out[20..40], &[0xBB; 20]);
        assert_eq!(&out[40..], &[0xCC; 10]);
        mock.assert_drained();
    }

    #[test]
    fn read_rejects_address_past_chip_end() {
        let c = chip("24C02", 256, 8, 1, 0);
        let mut mock = MockI2c::new([]);
        let err = read(&mut mock, &c, 250, 10, 0).unwrap_err();
        assert!(matches!(err, Error::AddressOutOfRange { .. }));
    }

    #[test]
    fn write_aligned_single_page() {
        let c = chip("24C02", 256, 8, 1, 0);
        let mut mock = MockI2c::new([Step {
            slave: 0x50,
            tx: vec![0x00, 0x11, 0x22, 0x33, 0x44],
            rx_len: 0,
            read_back: vec![],
            nack: false,
        }]);
        write(&mut mock, &c, 0x00, &[0x11, 0x22, 0x33, 0x44], 0).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn write_splits_at_page_boundary() {
        // 24C02: 8-byte pages. Starting at addr 0x06, a 10-byte write
        // should split into 2 bytes (filling page 0) + 8 bytes (page 1).
        let c = chip("24C02", 256, 8, 1, 0);
        let data: Vec<u8> = (0..10).collect();
        let mut mock = MockI2c::new([
            Step {
                slave: 0x50,
                tx: vec![0x06, 0, 1],
                rx_len: 0,
                read_back: vec![],
                nack: false,
            },
            Step {
                slave: 0x50,
                tx: vec![0x08, 2, 3, 4, 5, 6, 7, 8, 9],
                rx_len: 0,
                read_back: vec![],
                nack: false,
            },
        ]);
        write(&mut mock, &c, 0x06, &data, 0).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn write_splits_at_ch341_packet_limit() {
        // 24C256: 64-byte pages, but our CH341 chunk cap is 16 bytes —
        // a single in-page 32-byte write should still split into two
        // 16-byte transactions.
        let c = chip("24C256", 32768, 64, 2, 0);
        let data: Vec<u8> = (0..32).collect();
        let mut expected_steps = Vec::new();
        for i in 0..2 {
            let mut tx = vec![0x00, (i * 16) as u8];
            tx.extend((i * 16..i * 16 + 16).map(|x| x as u8));
            expected_steps.push(Step {
                slave: 0x50,
                tx,
                rx_len: 0,
                read_back: vec![],
                nack: false,
            });
        }
        let mut mock = MockI2c::new(expected_steps);
        write(&mut mock, &c, 0, &data, 0).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn write_rejects_address_past_chip_end() {
        let c = chip("24C02", 256, 8, 1, 0);
        let mut mock = MockI2c::new([]);
        let err = write(&mut mock, &c, 250, &[0; 10], 0).unwrap_err();
        assert!(matches!(err, Error::AddressOutOfRange { .. }));
    }
}
