//! CH341A USB protocol layer.
//!
//! Pin map (CH341A "black module" SPI mode):
//!   D0 = SCS    (chip-select, active low)
//!   D1 = CS1    (unused, held high)
//!   D2 = CS2    (unused, held high)
//!   D3 = SCK    (clock)
//!   D5 = MOSI   (data to flash)
//!   D7 = MISO   (data from flash)
//!
//! Pin map (CH341A I²C mode):
//!   D0 = SCL    (clock, open-drain via pull-ups)
//!   D1 = SDA    (data,  open-drain via pull-ups)
//!
//! Wire-level quirk (SPI only): the CH341 clocks bytes LSB-first; SPI
//! NOR is MSB-first, so every outgoing and incoming byte is bit-reversed.

use crate::error::{Error, Result};
use crate::i2c::I2cTransport;
use crate::spi::SpiTransport;
use rusb::{Context, DeviceHandle, UsbContext};
use std::time::Duration;

pub const VID: u16 = 0x1A86;
pub const PID: u16 = 0x5512;

/// SPI clock rates the `set_clock` command can select. Higher CH341
/// rates exist but require vendor commands etch341 doesn't implement.
pub const SUPPORTED_SPEEDS_KHZ: &[u32] = &[20, 100, 400, 750];

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
const CMD_I2C_STREAM: u8 = 0xAA;

// I²C-stream sub-commands. The set-speed command rides on the I²C
// stream even though we're driving SPI — the CH341 multiplexes
// clock control through this command.
const I2C_STM_SET: u8 = 0x60;
const I2C_STM_END: u8 = 0x00;
const I2C_STM_STA: u8 = 0x74;
const I2C_STM_STO: u8 = 0x75;
/// Base byte for "write N bytes"; low 5 bits = N, max 31 (0x9F).
const I2C_STM_OUT: u8 = 0x80;
/// Base byte for "read N bytes"; low 5 bits = N, max 31 (0xDF).
const I2C_STM_IN: u8 = 0xC0;

// UIO sub-commands (OR'd with a 6-bit payload)
const UIO_STM_END: u8 = 0x20;
const UIO_STM_DIR: u8 = 0x40;
const UIO_STM_OUT: u8 = 0x80;

/// D0 = SCS. Bit is set in the OUT payload to drive CS high (idle).
const CS_BIT: u8 = 1 << 0;

/// All three CS lines high (D0..D2 = 1), SCK low (D3 = 0),
/// DOUT lines high (D4..D5 = 1). Standard SPI idle state.
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
        let mut ch = Self::open_raw(verbose)?;
        ch.enable_spi_pins()?;
        Ok(ch)
    }

    /// Open the CH341A without configuring SPI pins. Use this when
    /// you want the device for I²C — the I2C_STREAM sub-commands
    /// manage SDA/SCL directly, so no UIO pin-config is needed.
    pub fn open_i2c(verbose: bool) -> Result<Self> {
        Self::open_raw(verbose)
    }

    fn open_raw(verbose: bool) -> Result<Self> {
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

        Ok(Self { handle, verbose })
    }

    /// Set SPI clock to one of [`SUPPORTED_SPEEDS_KHZ`]. Returns
    /// `Error::UnsupportedSpeed` for any other rate. No-op-safe to
    /// call multiple times; safe to call before or after `enable_spi_pins`.
    pub fn set_clock(&self, khz: u32) -> Result<()> {
        let bits = match khz {
            20 => 0u8,
            100 => 1,
            400 => 2,
            750 => 3,
            other => return Err(Error::UnsupportedSpeed(other)),
        };
        let buf = [CMD_I2C_STREAM, I2C_STM_SET | bits, I2C_STM_END];
        self.bulk_out(&buf)
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

// ---------------------------------------------------------------
// I²C transport
// ---------------------------------------------------------------
//
// CH341A I²C-stream model: a single USB OUT packet builds an entire
// I²C transaction from substream commands — `STA` (start), `OUT|N`
// (write N data bytes), `IN|N` (read N data bytes), `STO` (stop).
//
// **Important observed behaviour:** the CH341 does NOT return any
// IN-endpoint data for `OUT` substreams — no per-byte ACK status, no
// per-OUT-byte echo. The IN endpoint produces bytes only when an
// `IN|N` substream actually reads N bytes off the bus. Early drafts
// of this code assumed the CH341 echoed ACK status; that caused
// every probe to time out (silicon evidence: verbose-mode scan
// showed OUTs going out, zero IN replies).
//
//   - 7-bit slave addresses are shifted left by 1 on the wire to
//     make room for the R/W bit (W = 0, R = 1).
//   - Probe detects presence by reading 1 byte after addressing the
//     slave for *read*: if SDA is pulled high (no slave), we get
//     0xFF; if a slave drives it, we get whatever it sends. False
//     positives are possible for a slave whose current pointer
//     happens to return 0xFF, but for an EEPROM bus scan this is
//     close enough — and dramatically better than nothing.

impl Ch341 {
    /// Build the I²C transaction stream for `i2c_transfer`. Pure-write
    /// (rx_len == 0) skips the restart-read half; pure-read (tx empty,
    /// rx_len > 0) skips the write half. Returns the bytes ready to
    /// send to the CH341 OUT endpoint plus the number of bytes to
    /// expect from IN (just `rx_len` — `OUT` substreams don't produce
    /// IN traffic).
    fn build_i2c_stream(slave_7bit: u8, tx: &[u8], rx_len: usize) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(8 + tx.len());
        out.push(CMD_I2C_STREAM);
        // ---- write phase ----
        if !tx.is_empty() || rx_len == 0 {
            out.push(I2C_STM_STA);
            out.push(I2C_STM_OUT | (1 + tx.len() as u8));
            out.push(slave_7bit << 1); // W bit = 0
            out.extend_from_slice(tx);
        }
        // ---- restart-read phase ----
        if rx_len > 0 {
            out.push(I2C_STM_STA);
            out.push(I2C_STM_OUT | 1);
            out.push((slave_7bit << 1) | 0x01); // R bit = 1
            out.push(I2C_STM_IN | (rx_len as u8));
        }
        out.push(I2C_STM_STO);
        out.push(I2C_STM_END);

        (out, rx_len)
    }
}

impl I2cTransport for Ch341 {
    fn i2c_transfer(&mut self, slave_7bit: u8, tx: &[u8], rx_len: usize) -> Result<Vec<u8>> {
        let (out, in_total) = Self::build_i2c_stream(slave_7bit, tx, rx_len);
        self.bulk_out(&out)?;
        if in_total == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0u8; in_total];
        self.bulk_in(&mut buf)?;
        Ok(buf)
    }

    fn i2c_probe(&mut self, slave_7bit: u8) -> Result<bool> {
        // Address the slave for *read* and grab 1 byte. With no
        // device present, SDA stays pulled-up between bits and we
        // get 0xFF back; with a device present we typically get
        // either real data or 0x00. The 0xFF heuristic is the same
        // one most CH341 userspace tools (ch341eeprom, etc.) use
        // because the CH341 doesn't expose NACK detection directly.
        let buf = [
            CMD_I2C_STREAM,
            I2C_STM_STA,
            I2C_STM_OUT | 1,
            (slave_7bit << 1) | 0x01,
            I2C_STM_IN | 1,
            I2C_STM_STO,
            I2C_STM_END,
        ];
        self.bulk_out(&buf)?;
        let mut resp = [0u8; 1];
        match self.bulk_in(&mut resp) {
            Ok(()) => Ok(resp[0] != 0xFF),
            Err(_) => Ok(false),
        }
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
