//! SPI NOR flash protocol.
//!
//! The transport (CH341A or a mock) implements [`SpiTransport`]; this
//! module is hardware-agnostic and only knows about SPI bytes.

use crate::error::{Error, Result};
use std::time::{Duration, Instant};

pub mod opcode {
    pub const JEDEC_ID: u8 = 0x9F;
    pub const READ_STATUS: u8 = 0x05;
    /// Read Status Register 2 (W25Q-family + most modern Macronix /
    /// GigaDevice parts). Older / simpler chips (24C, SST25VF, some
    /// EN25 sizes) don't implement this — they NACK or echo back
    /// `0xFF`/`0x00`. Caller should treat suspicious-looking values
    /// as "not present" rather than as legit register state.
    pub const READ_STATUS_2: u8 = 0x35;
    /// Read Status Register 3 (W25Q-family). Even more chip-specific
    /// than SR2 — only the Winbond W25Q\* line uses this convention.
    /// Same "treat 0xFF as not-present" caveat applies.
    pub const READ_STATUS_3: u8 = 0x15;
    /// Read Serial Flash Discoverable Parameters table (JESD216).
    /// Modern SPI NOR (2011+) carries a self-describing block of
    /// metadata that lets us derive size / page / sector / opcode
    /// info without a DB lookup. Wire format is opcode + 3-byte
    /// big-endian SFDP address + 1 dummy byte, then read N bytes.
    pub const READ_SFDP: u8 = 0x5A;
    /// Read Security Register (Winbond W25Q / GigaDevice GD25Q
    /// convention). Same wire shape as SFDP: opcode + 3-byte
    /// big-endian address + 1 dummy byte, then read N bytes. The
    /// address's bits A13..A8 select the register (0x1000 / 0x2000 /
    /// 0x3000 for registers 1 / 2 / 3) and A7..A0 the byte offset
    /// inside it. Macronix uses a different opcode (0x2B) for its
    /// single security register, so this read is meaningful only on
    /// the 0x48-convention families.
    pub const READ_SECURITY_REG: u8 = 0x48;
    /// Program Security Register (0x42) and Erase Security Register
    /// (0x44), same 0x48-convention families. Both need a Write
    /// Enable first and are polled via SR1.WIP. Repeatable until the
    /// register's lock bit (LB in SR2) is set — etch341 never sets
    /// that bit, so erase + reprogram stays available.
    pub const PROGRAM_SECURITY_REG: u8 = 0x42;
    pub const ERASE_SECURITY_REG: u8 = 0x44;
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
    /// Write-Enable Latch. Set by 0x06 (WREN), cleared after every
    /// program/erase op. Not currently checked by etch341 — kept as
    /// documented API for future "verify the chip accepted WREN"
    /// helpers and for the integration tests.
    #[allow(dead_code)]
    pub const WEL: u8 = 0b0000_0010;
}

/// Raw SR1/SR2/SR3 register bytes read from the chip, plus the
/// canonical W25Q-family decoded bit names. Other vendors mostly
/// follow this layout for SR1; SR2/SR3 vary more — the decode is
/// best-effort and shouldn't be used to drive writes on chips
/// outside the W25Q / MX25L / GD25Q families without checking the
/// datasheet first.
#[derive(Debug, Clone, Copy)]
pub struct StatusRegisters {
    pub sr1: u8,
    pub sr2: u8,
    pub sr3: u8,
}

impl StatusRegisters {
    // ---- SR1 ----
    pub fn wip(self) -> bool {
        self.sr1 & 0b0000_0001 != 0
    }
    pub fn wel(self) -> bool {
        self.sr1 & 0b0000_0010 != 0
    }
    /// Block-Protect bits (BP0..BP2). Together with TB and SR1.bit6
    /// (varies BP3/SEC by family) they decode to a protected
    /// region. We surface the raw value; printing converts to
    /// dot/bar style for readability.
    pub fn bp(self) -> u8 {
        (self.sr1 >> 2) & 0b0000_0111
    }
    /// Top/Bottom protect. When set, the BP bits protect from the
    /// bottom of the chip up; when clear, from the top down.
    pub fn tb(self) -> bool {
        self.sr1 & 0b0010_0000 != 0
    }
    /// Bit 6 of SR1. On W25Q this is SEC (4 KB sector vs 64 KB
    /// block protect granularity); on MX25L it's BP3 (extending
    /// the block-protect mask one bit). Caller decides which name
    /// to print based on chip family.
    pub fn sec_or_bp3(self) -> bool {
        self.sr1 & 0b0100_0000 != 0
    }
    /// Status Register Protect 0 — pairs with SR2.SRP1 to gate
    /// whether SR can be written via WRSR.
    pub fn srp0(self) -> bool {
        self.sr1 & 0b1000_0000 != 0
    }

    // ---- SR2 ----
    pub fn srp1(self) -> bool {
        self.sr2 & 0b0000_0001 != 0
    }
    /// Quad Enable. Must be set for chips wired in QSPI mode; some
    /// devices NACK the read-quad opcodes if it's clear.
    pub fn qe(self) -> bool {
        self.sr2 & 0b0000_0010 != 0
    }
    /// Security-register lock bits LB1..LB3. Setting one bit
    /// one-time-programs that security page closed.
    pub fn lb(self) -> u8 {
        (self.sr2 >> 3) & 0b0000_0111
    }
    /// Complement Protect — inverts the BP protect range
    /// (protected region becomes unprotected and vice versa).
    pub fn cmp(self) -> bool {
        self.sr2 & 0b0100_0000 != 0
    }
    /// Erase / Program Suspend status.
    pub fn sus(self) -> bool {
        self.sr2 & 0b1000_0000 != 0
    }

    // ---- SR3 ----
    /// 4-byte-address mode at power-up.
    pub fn adp(self) -> bool {
        self.sr3 & 0b0000_0010 != 0
    }
    /// Write Protect Selection: 0 = CMP/SEC/TB/BP (standard);
    /// 1 = individual block / sector lock (WPS-style).
    pub fn wps(self) -> bool {
        self.sr3 & 0b0000_0100 != 0
    }
    /// Output driver strength (DRV1:DRV0). 0=100%, 1=75%, 2=50%, 3=25%.
    pub fn drv(self) -> u8 {
        (self.sr3 >> 5) & 0b0000_0011
    }
    /// Pin-7 mode: 0 = /HOLD enabled; 1 = /RESET enabled.
    pub fn hold_rst(self) -> bool {
        self.sr3 & 0b1000_0000 != 0
    }

    /// `true` when the byte is `0xFF` — every read returned all-ones
    /// which on SPI means MISO was high-z (no device drove it) or
    /// the chip NACKed by tri-stating. Useful for hiding garbage
    /// decode of SR2/SR3 on chips that don't implement those.
    pub fn sr2_present(self) -> bool {
        self.sr2 != 0xFF
    }
    pub fn sr3_present(self) -> bool {
        self.sr3 != 0xFF
    }
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

pub fn read_status_2(spi: &mut dyn SpiTransport) -> Result<u8> {
    let rx = spi.spi_transfer(&[opcode::READ_STATUS_2, 0])?;
    Ok(rx[1])
}

pub fn read_status_3(spi: &mut dyn SpiTransport) -> Result<u8> {
    let rx = spi.spi_transfer(&[opcode::READ_STATUS_3, 0])?;
    Ok(rx[1])
}

/// Read `len` bytes from the chip's SFDP space starting at the
/// 24-bit `offset`. SFDP is a separate address space from main
/// flash; the wire sequence is `0x5A`, three big-endian address
/// bytes, one dummy byte (clocked while the chip aligns its
/// internal pointer), then the payload bytes. Returns the payload
/// only (the leading 5 command/dummy bytes are stripped).
///
/// Chips made before JESD216 was widespread (pre-2011) typically
/// return `0xFF` for every byte — the caller should treat the
/// leading bytes as "missing SFDP" if they don't decode as the
/// "SFDP" magic.
pub fn read_sfdp(spi: &mut dyn SpiTransport, offset: u32, len: usize) -> Result<Vec<u8>> {
    let mut cmd = Vec::with_capacity(5 + len);
    cmd.push(opcode::READ_SFDP);
    cmd.push((offset >> 16) as u8);
    cmd.push((offset >> 8) as u8);
    cmd.push(offset as u8);
    cmd.push(0); // dummy
    cmd.resize(5 + len, 0);
    let mut rx = spi.spi_transfer(&cmd)?;
    rx.drain(..5);
    Ok(rx)
}

/// Read `len` bytes from a security register starting at `addr`
/// (the register-selecting 24-bit address, e.g. `0x1000` for
/// register 1). Wire format matches SFDP: opcode + 3-byte address +
/// 1 dummy byte, then the payload. The leading 5 command/dummy
/// bytes are stripped. Read-only — the program/erase opcodes
/// (`0x42` / `0x44`) are deliberately not implemented yet because
/// they're one-time and want their own arm/confirm flow.
pub fn read_security_register(
    spi: &mut dyn SpiTransport,
    addr: u32,
    len: usize,
) -> Result<Vec<u8>> {
    let mut cmd = Vec::with_capacity(5 + len);
    cmd.push(opcode::READ_SECURITY_REG);
    cmd.push((addr >> 16) as u8);
    cmd.push((addr >> 8) as u8);
    cmd.push(addr as u8);
    cmd.push(0); // dummy
    cmd.resize(5 + len, 0);
    let mut rx = spi.spi_transfer(&cmd)?;
    rx.drain(..5);
    Ok(rx)
}

/// Read all three status registers in a single op. SR2/SR3 reads
/// against a chip that doesn't implement them return `0xFF` (MISO
/// pulled high while the chip ignores the opcode); `StatusRegisters`
/// surfaces that via `sr2_present`/`sr3_present` so the caller can
/// skip the decode for that register.
pub fn read_all_status(spi: &mut dyn SpiTransport) -> Result<StatusRegisters> {
    Ok(StatusRegisters {
        sr1: read_status(spi)?,
        sr2: read_status_2(spi)?,
        sr3: read_status_3(spi)?,
    })
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

/// Erase one security register (opcode 0x44) back to 0xFF. `addr` is
/// the register-select address (0x1000 / 0x2000 / 0x3000). Issues
/// Write Enable first; the caller polls SR1.WIP for completion.
/// Always 3-byte addressed — the security-register window is tiny
/// and well inside 24 bits even on >16 MB parts.
pub fn erase_security_register(spi: &mut dyn SpiTransport, addr: u32) -> Result<()> {
    write_enable(spi)?;
    let mut cmd = Vec::with_capacity(4);
    cmd.push(opcode::ERASE_SECURITY_REG);
    cmd.extend(addr24_be(addr));
    spi.spi_transfer(&cmd)?;
    Ok(())
}

/// Program up to 256 bytes into a security register (opcode 0x42)
/// starting at `addr`. Like any program it only clears bits
/// (1 -> 0), so the register wants an erase first for a clean write.
/// Issues Write Enable first; the caller polls SR1.WIP. 3-byte
/// addressed for the same reason as [`erase_security_register`].
pub fn program_security_register(spi: &mut dyn SpiTransport, addr: u32, data: &[u8]) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    write_enable(spi)?;
    let mut cmd = Vec::with_capacity(4 + data.len());
    cmd.push(opcode::PROGRAM_SECURITY_REG);
    cmd.extend(addr24_be(addr));
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
    fn read_security_register_sends_48_plus_addr_plus_dummy_and_strips_header() {
        // Register 1 lives at 0x001000: opcode 0x48, 3-byte address,
        // 1 dummy byte, then `len` read bytes. The 5-byte header is
        // stripped from the returned payload.
        let mut mock = MockSpi::new([(
            vec![0x48, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00],
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xDE, 0xAD, 0xBE],
        )]);
        assert_eq!(
            read_security_register(&mut mock, 0x1000, 3).unwrap(),
            vec![0xDE, 0xAD, 0xBE]
        );
        mock.assert_drained();
    }

    #[test]
    fn erase_security_register_sends_wren_then_44_plus_addr() {
        // Register 2 lives at 0x002000.
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (vec![0x44, 0x00, 0x20, 0x00], vec![0x00; 4]),
        ]);
        erase_security_register(&mut mock, 0x2000).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn program_security_register_sends_wren_then_42_plus_addr_plus_data() {
        // Register 3 @ 0x003000, offset 0x10 → 0x003010.
        let mut mock = MockSpi::new([
            (vec![0x06], vec![0x00]),
            (
                vec![0x42, 0x00, 0x30, 0x10, 0xDE, 0xAD, 0xBE, 0xEF],
                vec![0x00; 8],
            ),
        ]);
        program_security_register(&mut mock, 0x3010, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        mock.assert_drained();
    }

    #[test]
    fn program_security_register_empty_is_noop() {
        let mut mock = MockSpi::new([]);
        program_security_register(&mut mock, 0x1000, &[]).unwrap();
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
