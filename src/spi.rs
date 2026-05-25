//! SPI NOR flash protocol.
//!
//! The transport (CH341A or a mock) implements [`SpiTransport`]; this
//! module is hardware-agnostic and only knows about SPI bytes.

use crate::error::{Error, Result};
use std::time::{Duration, Instant};

pub mod opcode {
    pub const JEDEC_ID: u8 = 0x9F;
    pub const READ_STATUS: u8 = 0x05;
    pub const WRITE_ENABLE: u8 = 0x06;
    pub const CHIP_ERASE: u8 = 0xC7;
    pub const SECTOR_ERASE_4K: u8 = 0x20;
    pub const BLOCK_ERASE_64K: u8 = 0xD8;
    pub const PAGE_PROGRAM: u8 = 0x02;
    pub const READ_DATA: u8 = 0x03;
}

pub mod sr1 {
    pub const WIP: u8 = 0b0000_0001;
    pub const WEL: u8 = 0b0000_0010;
}

/// One full-duplex SPI transaction: assert CS, clock `tx` out / capture
/// `tx.len()` bytes from MISO, deassert CS. Implementors handle bit
/// ordering and chunking.
pub trait SpiTransport {
    fn spi_transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>>;
}

pub fn jedec_read(spi: &mut dyn SpiTransport) -> Result<[u8; 3]> {
    let rx = spi.spi_transfer(&[opcode::JEDEC_ID, 0, 0, 0])?;
    Ok([rx[1], rx[2], rx[3]])
}

pub fn read_status(spi: &mut dyn SpiTransport) -> Result<u8> {
    let rx = spi.spi_transfer(&[opcode::READ_STATUS, 0])?;
    Ok(rx[1])
}

pub fn write_enable(spi: &mut dyn SpiTransport) -> Result<()> {
    spi.spi_transfer(&[opcode::WRITE_ENABLE])?;
    Ok(())
}

/// Poll SR1.WIP until clear, or until `timeout` elapses.
pub fn wait_until_ready(spi: &mut dyn SpiTransport, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if read_status(spi)? & sr1::WIP == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Read `len` bytes starting at 24-bit `addr`. Chips > 16 MB need
/// 4-byte addressing (opcode 0x13) which isn't implemented yet.
pub fn read_data(spi: &mut dyn SpiTransport, addr: u32, len: usize) -> Result<Vec<u8>> {
    let mut cmd = Vec::with_capacity(4 + len);
    cmd.push(opcode::READ_DATA);
    cmd.extend(addr24_be(addr));
    cmd.resize(4 + len, 0);
    let mut rx = spi.spi_transfer(&cmd)?;
    rx.drain(..4);
    Ok(rx)
}

pub fn chip_erase(spi: &mut dyn SpiTransport) -> Result<()> {
    write_enable(spi)?;
    spi.spi_transfer(&[opcode::CHIP_ERASE])?;
    Ok(())
}

pub fn sector_erase_4k(spi: &mut dyn SpiTransport, addr: u32) -> Result<()> {
    write_enable(spi)?;
    let [a2, a1, a0] = addr24_be(addr);
    spi.spi_transfer(&[opcode::SECTOR_ERASE_4K, a2, a1, a0])?;
    Ok(())
}

pub fn block_erase_64k(spi: &mut dyn SpiTransport, addr: u32) -> Result<()> {
    write_enable(spi)?;
    let [a2, a1, a0] = addr24_be(addr);
    spi.spi_transfer(&[opcode::BLOCK_ERASE_64K, a2, a1, a0])?;
    Ok(())
}

/// Page-program up to 256 bytes; must not cross a 256-byte page boundary.
pub fn page_program(
    spi: &mut dyn SpiTransport,
    addr: u32,
    data: &[u8],
    page_size: u32,
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let page_offset = addr % page_size;
    if page_offset as usize + data.len() > page_size as usize {
        return Err(Error::PageBoundaryCrossing {
            addr,
            len: data.len(),
            page_size,
        });
    }
    write_enable(spi)?;
    let mut cmd = Vec::with_capacity(4 + data.len());
    cmd.push(opcode::PAGE_PROGRAM);
    cmd.extend(addr24_be(addr));
    cmd.extend_from_slice(data);
    spi.spi_transfer(&cmd)?;
    Ok(())
}

fn addr24_be(addr: u32) -> [u8; 3] {
    [(addr >> 16) as u8, (addr >> 8) as u8, addr as u8]
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::{Result, SpiTransport};
    use std::collections::VecDeque;

    /// Deterministic SPI mock: each [`spi_transfer`] call must match the
    /// next expected `tx` and yields the paired `rx`. Test fails loudly
    /// on a mismatch or an unexpected call.
    pub struct MockSpi {
        steps: VecDeque<(Vec<u8>, Vec<u8>)>,
    }

    impl MockSpi {
        pub fn new(steps: impl IntoIterator<Item = (Vec<u8>, Vec<u8>)>) -> Self {
            Self {
                steps: steps.into_iter().collect(),
            }
        }

        pub fn assert_drained(&self) {
            assert!(
                self.steps.is_empty(),
                "{} expected SPI interactions were never consumed",
                self.steps.len()
            );
        }
    }

    impl SpiTransport for MockSpi {
        fn spi_transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>> {
            let (expected, rx) = self
                .steps
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected spi_transfer({})", hex::encode(tx)));
            assert_eq!(
                tx,
                expected.as_slice(),
                "SPI tx mismatch:\n  got: {}\n  expected: {}",
                hex::encode(tx),
                hex::encode(&expected)
            );
            Ok(rx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::MockSpi;

    #[test]
    fn jedec_read_sends_9f_and_returns_3_bytes() {
        let mut mock = MockSpi::new([(vec![0x9F, 0x00, 0x00, 0x00], vec![0xFF, 0xEF, 0x40, 0x18])]);
        assert_eq!(jedec_read(&mut mock).unwrap(), [0xEF, 0x40, 0x18]);
        mock.assert_drained();
    }

    #[test]
    fn read_data_builds_command_and_strips_header() {
        let mut mock = MockSpi::new([(
            vec![0x03, 0x01, 0x23, 0x45, 0x00, 0x00, 0x00],
            vec![0x00, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE],
        )]);
        assert_eq!(
            read_data(&mut mock, 0x012345, 3).unwrap(),
            vec![0xDE, 0xAD, 0xBE]
        );
        mock.assert_drained();
    }

    #[test]
    fn sector_erase_sends_wren_then_20_plus_addr() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x20, 0x00, 0x10, 0x00], vec![0x00, 0x00, 0x00, 0x00]),
        ]);
        sector_erase_4k(&mut mock, 0x1000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn block_erase_sends_wren_then_d8_plus_addr() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0xD8, 0x01, 0x00, 0x00], vec![0x00, 0x00, 0x00, 0x00]),
        ]);
        block_erase_64k(&mut mock, 0x10000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn chip_erase_sends_wren_then_c7() {
        let mut mock = MockSpi::new([(vec![0x06], vec![0x00]), (vec![0xC7], vec![0x00])]);
        chip_erase(&mut mock).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn page_program_within_page() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x02, 0x00, 0x00, 0x80, 0xAA, 0xBB, 0xCC], vec![0; 7]),
        ]);
        page_program(&mut mock, 0x80, &[0xAA, 0xBB, 0xCC], 256).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn page_program_rejects_boundary_crossing() {
        let mut mock = MockSpi::new([]);
        let r = page_program(&mut mock, 0xFE, &[0, 0, 0, 0], 256);
        assert!(matches!(r, Err(Error::PageBoundaryCrossing { .. })));
    }

    #[test]
    fn wait_until_ready_returns_when_wip_clears() {
        let mut mock = MockSpi::new([
            (vec![0x05, 0x00], vec![0x00, 0x01]), // WIP set
            (vec![0x05, 0x00], vec![0x00, 0x01]), // WIP still set
            (vec![0x05, 0x00], vec![0x00, 0x00]), // WIP clear
        ]);
        wait_until_ready(&mut mock, Duration::from_secs(1)).unwrap();
        mock.assert_drained();
    }
}
