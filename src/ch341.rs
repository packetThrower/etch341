//! CH341A USB protocol layer.
//!
//! Reference: flashrom/programmers/ch341a_spi.c (canonical).
//!
//! Pin map (CH341A "black module" SPI mode):
//!   D0 = SCS    (chip-select, active low)
//!   D1 = CS1    (unused, held high)
//!   D2 = CS2    (unused, held high)
//!   D3 = SCK    (clock)
//!   D5 = MOSI   (data to flash)
//!   D7 = MISO   (data from flash)
//!
//! Wire-level quirk: the CH341 clocks bytes LSB-first; SPI NOR is
//! MSB-first, so every outgoing and incoming byte is bit-reversed.

use crate::error::{Error, Result};
use crate::spi::SpiTransport;
use rusb::{Context, DeviceHandle, UsbContext};
use std::time::Duration;

pub const VID: u16 = 0x1A86;
pub const PID: u16 = 0x5512;

pub const SUPPORTED_SPEEDS_KHZ: &[u32] = &[400, 750, 1500, 3000, 6000, 12000, 24000];

const INTERFACE: u8 = 0;
const EP_OUT: u8 = 0x02;
const EP_IN: u8 = 0x82;
const USB_TIMEOUT: Duration = Duration::from_millis(1000);

/// CH341 USB FIFO is 32 bytes per packet; one byte is the command header.
const PACKET_LEN: usize = 0x20;
const MAX_PAYLOAD_PER_PKT: usize = PACKET_LEN - 1;

// Stream command opcodes
const CMD_SPI_STREAM: u8 = 0xA8;
const CMD_UIO_STREAM: u8 = 0xAB;

// UIO sub-commands (OR'd with a 6-bit payload)
const UIO_STM_END: u8 = 0x20;
const UIO_STM_DIR: u8 = 0x40;
const UIO_STM_OUT: u8 = 0x80;

/// D0 = SCS. Bit is set in the OUT payload to drive CS high (idle).
const CS_BIT: u8 = 1 << 0;

/// All three CS lines high (D0..D2 = 1), SCK low (D3 = 0),
/// DOUT lines high (D4..D5 = 1). Matches flashrom's "idle" state.
const PIN_IDLE: u8 = 0x37;

/// Direction byte for SPI mode: D0..D5 outputs, D6..D7 inputs
/// (D7 is MISO).
const PIN_DIR_SPI: u8 = 0x3F;

pub struct Ch341 {
    handle: DeviceHandle<Context>,
    verbose: bool,
}

impl Ch341 {
    pub fn open(verbose: bool) -> Result<Self> {
        let ctx = Context::new()?;
        let handle = ctx
            .open_device_with_vid_pid(VID, PID)
            .ok_or(Error::DeviceNotFound)?;

        // Linux only; macOS / Windows are no-ops or unsupported and we
        // don't care about the result either way.
        let _ = handle.set_auto_detach_kernel_driver(true);

        handle.claim_interface(INTERFACE).map_err(|e| match e {
            rusb::Error::Access => Error::PermissionDenied,
            rusb::Error::NotFound => Error::DeviceNotFound,
            other => Error::Usb(other),
        })?;

        let mut ch = Self { handle, verbose };
        ch.enable_spi_pins()?;
        Ok(ch)
    }

    fn enable_spi_pins(&mut self) -> Result<()> {
        let buf = [
            CMD_UIO_STREAM,
            UIO_STM_OUT | PIN_IDLE,
            UIO_STM_DIR | PIN_DIR_SPI,
            UIO_STM_END,
        ];
        self.bulk_out(&buf)
    }

    fn cs_assert(&mut self) -> Result<()> {
        let buf = [
            CMD_UIO_STREAM,
            UIO_STM_OUT | (PIN_IDLE & !CS_BIT), // 0x36
            UIO_STM_END,
        ];
        self.bulk_out(&buf)
    }

    fn cs_deassert(&mut self) -> Result<()> {
        let buf = [
            CMD_UIO_STREAM,
            UIO_STM_OUT | PIN_IDLE, // 0x37
            UIO_STM_END,
        ];
        self.bulk_out(&buf)
    }

    fn bulk_out(&self, buf: &[u8]) -> Result<()> {
        if self.verbose {
            eprintln!("  -> OUT  [{:>2}]: {}", buf.len(), hex::encode(buf));
        }
        let n = self.handle.write_bulk(EP_OUT, buf, USB_TIMEOUT)?;
        if n != buf.len() {
            return Err(Error::Usb(rusb::Error::Io));
        }
        Ok(())
    }

    fn bulk_in(&self, buf: &mut [u8]) -> Result<()> {
        let n = self.handle.read_bulk(EP_IN, buf, USB_TIMEOUT)?;
        if self.verbose {
            eprintln!("  <- IN   [{:>2}]: {}", n, hex::encode(&buf[..n]));
        }
        if n != buf.len() {
            return Err(Error::Usb(rusb::Error::Io));
        }
        Ok(())
    }
}

impl Drop for Ch341 {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(INTERFACE);
    }
}

impl SpiTransport for Ch341 {
    /// Full-duplex SPI: assert CS, clock out `tx`, capture `tx.len()`
    /// bytes from MISO, deassert CS. Returned bytes are bit-reversed
    /// back to MSB-first so callers see normal SPI data.
    fn spi_transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>> {
        self.cs_assert()?;
        let mut rx = Vec::with_capacity(tx.len());

        for chunk in tx.chunks(MAX_PAYLOAD_PER_PKT) {
            let mut pkt = Vec::with_capacity(1 + chunk.len());
            pkt.push(CMD_SPI_STREAM);
            pkt.extend(chunk.iter().map(|b| b.reverse_bits()));
            self.bulk_out(&pkt)?;

            let mut rbuf = vec![0u8; chunk.len()];
            self.bulk_in(&mut rbuf)?;
            for b in &mut rbuf {
                *b = b.reverse_bits();
            }
            rx.extend_from_slice(&rbuf);
        }

        self.cs_deassert()?;
        Ok(rx)
    }
}
