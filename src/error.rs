//! Typed errors used across the crate.

use thiserror::Error;

// A few variants below (WriteProtected, ChipDb, I2cNack) aren't
// constructed yet — they're modeled now for the SR1.SRP / .toml
// parse-failure / I²C-NACK paths that callers want a typed handle
// for as soon as the underlying detection lands. Until then, the
// dead-code lint would gate every CI run on a manual-wiring task
// each variant doesn't need.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum Error {
    #[error("CH341A device not found (looking for USB 1a86:5512). Is it plugged in?")]
    DeviceNotFound,

    #[error(
        "Permission denied opening CH341A.\n  \
         Linux: sudo cp platform/udev/99-ch341a.rules /etc/udev/rules.d/ && sudo udevadm control --reload\n  \
         Windows: run Zadig and bind WinUSB to the CH341A device."
    )]
    PermissionDenied,

    #[error("Chip with JEDEC ID {0} not in database; pass --chip <NAME> to override")]
    ChipNotRecognized(String),

    #[error("Verify failed at 0x{addr:08X}: expected 0x{expected:02X}, got 0x{actual:02X}")]
    VerifyFailed { addr: u32, expected: u8, actual: u8 },

    #[error("Chip is write-protected (SR1 BP/SRP bits set); clear protection and retry")]
    WriteProtected,

    #[error("Unsupported SPI clock {0} kHz; supported: 400, 750, 1500, 3000, 6000, 12000, 24000")]
    UnsupportedSpeed(u32),

    #[error("USB error: {0}")]
    Usb(#[from] rusb::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Chip DB error in {path}: {source}")]
    ChipDb {
        path: String,
        #[source]
        source: toml::de::Error,
    },

    #[error("Timed out waiting for chip to become ready (WIP stayed set)")]
    Timeout,

    #[error(
        "Address out of range for chip (chip 0x{chip_size:X} B): start=0x{addr:08X} len=0x{len:X}"
    )]
    AddressOutOfRange { addr: u32, len: u32, chip_size: u32 },

    #[error("Erase start 0x{addr:08X} is not aligned to sector size {sector_size}")]
    UnalignedErase { addr: u32, sector_size: u32 },

    #[error("Page program crosses a {page_size}-byte page boundary: addr=0x{addr:08X} len={len}")]
    PageBoundaryCrossing {
        addr: u32,
        len: usize,
        page_size: u32,
    },

    #[error("Chip is not blank at 0x{addr:08X} (read 0x{value:02X})")]
    NotBlank { addr: u32, value: u8 },

    #[error("I²C device at 0x{slave_7bit:02X} did not ACK")]
    I2cNack { slave_7bit: u8 },
}

pub type Result<T> = std::result::Result<T, Error>;
