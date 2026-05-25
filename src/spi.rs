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
    // Standard 3-byte addressing opcodes (chips ≤ 16 MB).
    pub const SECTOR_ERASE_4K: u8 = 0x20;
    pub const BLOCK_ERASE_64K: u8 = 0xD8;
    pub const PAGE_PROGRAM: u8 = 0x02;
    pub const READ_DATA: u8 = 0x03;
    // 4-byte-address variants (chips > 16 MB). These avoid the
    // global mode-switch (0xB7 / 0xE9) which persists across ops
    // and bites you if a panic skips the mode-exit.
    pub const SECTOR_ERASE_4K_4B: u8 = 0x21;
    pub const BLOCK_ERASE_64K_4B: u8 = 0xDC;
    pub const PAGE_PROGRAM_4B: u8 = 0x12;
    pub const READ_DATA_4B: u8 = 0x13;
}

/// Address width for chip-side opcodes. Picked per-op based on the
/// chip's size; 4-byte form uses dedicated opcode variants so we never
/// touch the chip's persistent mode bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Addressing {
    /// 24-bit addresses; chips ≤ 16 MB.
    ThreeByte,
    /// 32-bit addresses; chips > 16 MB. Uses 4-byte opcode variants.
    FourByte,
}

impl Addressing {
    fn read_op(self) -> u8 {
        match self {
            Self::ThreeByte => opcode::READ_DATA,
            Self::FourByte => opcode::READ_DATA_4B,
        }
    }
    fn page_program_op(self) -> u8 {
        match self {
            Self::ThreeByte => opcode::PAGE_PROGRAM,
            Self::FourByte => opcode::PAGE_PROGRAM_4B,
        }
    }
    fn sector_erase_op(self) -> u8 {
        match self {
            Self::ThreeByte => opcode::SECTOR_ERASE_4K,
            Self::FourByte => opcode::SECTOR_ERASE_4K_4B,
        }
    }
    fn block_erase_op(self) -> u8 {
        match self {
            Self::ThreeByte => opcode::BLOCK_ERASE_64K,
            Self::FourByte => opcode::BLOCK_ERASE_64K_4B,
        }
    }
    fn addr_bytes(self, addr: u32) -> Vec<u8> {
        match self {
            Self::ThreeByte => addr24_be(addr).to_vec(),
            Self::FourByte => addr32_be(addr).to_vec(),
        }
    }
    fn header_len(self) -> usize {
        match self {
            Self::ThreeByte => 4, // opcode + 3 address bytes
            Self::FourByte => 5,  // opcode + 4 address bytes
        }
    }
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

/// Read `len` bytes starting at `addr`. Caller picks the address
/// width to match the chip (`spi::Addressing::ThreeByte` for ≤ 16 MB,
/// `FourByte` for larger).
pub fn read_data(
    spi: &mut dyn SpiTransport,
    addressing: Addressing,
    addr: u32,
    len: usize,
) -> Result<Vec<u8>> {
    let addr_bytes = addressing.addr_bytes(addr);
    let header = 1 + addr_bytes.len();
    let mut cmd = Vec::with_capacity(header + len);
    cmd.push(addressing.read_op());
    cmd.extend(addr_bytes);
    cmd.resize(header + len, 0);
    let mut rx = spi.spi_transfer(&cmd)?;
    rx.drain(..header);
    Ok(rx)
}

pub fn chip_erase(spi: &mut dyn SpiTransport) -> Result<()> {
    write_enable(spi)?;
    spi.spi_transfer(&[opcode::CHIP_ERASE])?;
    Ok(())
}

pub fn sector_erase_4k(
    spi: &mut dyn SpiTransport,
    addressing: Addressing,
    addr: u32,
) -> Result<()> {
    write_enable(spi)?;
    let mut cmd = Vec::with_capacity(1 + addressing.addr_bytes(addr).len());
    cmd.push(addressing.sector_erase_op());
    cmd.extend(addressing.addr_bytes(addr));
    spi.spi_transfer(&cmd)?;
    Ok(())
}

pub fn block_erase_64k(
    spi: &mut dyn SpiTransport,
    addressing: Addressing,
    addr: u32,
) -> Result<()> {
    write_enable(spi)?;
    let mut cmd = Vec::with_capacity(1 + addressing.addr_bytes(addr).len());
    cmd.push(addressing.block_erase_op());
    cmd.extend(addressing.addr_bytes(addr));
    spi.spi_transfer(&cmd)?;
    Ok(())
}

/// Page-program up to 256 bytes; must not cross a 256-byte page boundary.
pub fn page_program(
    spi: &mut dyn SpiTransport,
    addressing: Addressing,
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
    let mut cmd = Vec::with_capacity(addressing.header_len() + data.len());
    cmd.push(addressing.page_program_op());
    cmd.extend(addressing.addr_bytes(addr));
    cmd.extend_from_slice(data);
    spi.spi_transfer(&cmd)?;
    Ok(())
}

fn addr24_be(addr: u32) -> [u8; 3] {
    [(addr >> 16) as u8, (addr >> 8) as u8, addr as u8]
}

fn addr32_be(addr: u32) -> [u8; 4] {
    [
        (addr >> 24) as u8,
        (addr >> 16) as u8,
        (addr >> 8) as u8,
        addr as u8,
    ]
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
    fn read_data_3byte_builds_command_and_strips_header() {
        let mut mock = MockSpi::new([(
            vec![0x03, 0x01, 0x23, 0x45, 0x00, 0x00, 0x00],
            vec![0x00, 0x00, 0x00, 0x00, 0xDE, 0xAD, 0xBE],
        )]);
        assert_eq!(
            read_data(&mut mock, Addressing::ThreeByte, 0x012345, 3).unwrap(),
            vec![0xDE, 0xAD, 0xBE]
        );
        mock.assert_drained();
    }

    #[test]
    fn read_data_4byte_uses_opcode_13_and_4_addr_bytes() {
        let mut mock = MockSpi::new([(
            vec![0x13, 0x01, 0x23, 0x45, 0x67, 0x00, 0x00, 0x00],
            vec![0x00, 0x00, 0x00, 0x00, 0x00, 0xCA, 0xFE, 0xBA],
        )]);
        assert_eq!(
            read_data(&mut mock, Addressing::FourByte, 0x01234567, 3).unwrap(),
            vec![0xCA, 0xFE, 0xBA]
        );
        mock.assert_drained();
    }

    #[test]
    fn sector_erase_3byte_sends_wren_then_20_plus_addr() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x20, 0x00, 0x10, 0x00], vec![0x00, 0x00, 0x00, 0x00]),
        ]);
        sector_erase_4k(&mut mock, Addressing::ThreeByte, 0x1000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn sector_erase_4byte_uses_opcode_21_and_4_addr_bytes() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x21, 0x01, 0x00, 0x10, 0x00], vec![0x00; 5]),
        ]);
        sector_erase_4k(&mut mock, Addressing::FourByte, 0x01001000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn block_erase_3byte_sends_wren_then_d8_plus_addr() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0xD8, 0x01, 0x00, 0x00], vec![0x00, 0x00, 0x00, 0x00]),
        ]);
        block_erase_64k(&mut mock, Addressing::ThreeByte, 0x10000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn block_erase_4byte_uses_opcode_dc_and_4_addr_bytes() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0xDC, 0x02, 0x00, 0x00, 0x00], vec![0x00; 5]),
        ]);
        block_erase_64k(&mut mock, Addressing::FourByte, 0x02000000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn chip_erase_sends_wren_then_c7() {
        let mut mock = MockSpi::new([(vec![0x06], vec![0x00]), (vec![0xC7], vec![0x00])]);
        chip_erase(&mut mock).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn page_program_3byte_within_page() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x02, 0x00, 0x00, 0x80, 0xAA, 0xBB, 0xCC], vec![0; 7]),
        ]);
        page_program(
            &mut mock,
            Addressing::ThreeByte,
            0x80,
            &[0xAA, 0xBB, 0xCC],
            256,
        )
        .unwrap();
        mock.assert_drained();
    }

    #[test]
    fn page_program_4byte_uses_opcode_12() {
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x12, 0x01, 0x00, 0x00, 0x00, 0xDE, 0xAD], vec![0; 7]),
        ]);
        page_program(
            &mut mock,
            Addressing::FourByte,
            0x01000000,
            &[0xDE, 0xAD],
            256,
        )
        .unwrap();
        mock.assert_drained();
    }

    #[test]
    fn page_program_rejects_boundary_crossing() {
        let mut mock = MockSpi::new([]);
        let r = page_program(&mut mock, Addressing::ThreeByte, 0xFE, &[0, 0, 0, 0], 256);
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
