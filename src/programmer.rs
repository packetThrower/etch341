//! Programmer dispatch layer.
//!
//! The high-level SPI / I²C operations in `ops`, `spi`, and `i2c` are
//! generic over the [`SpiTransport`] and [`I2cTransport`] traits, so
//! they don't care which USB bridge is on the other end. `Programmer`
//! is the runtime dispatch over the supported bridges: it owns the
//! concrete device and forwards the transport calls to it.
//!
//! Adding a new programmer (e.g. a CH347) is a new enum variant backed
//! by its own module implementing the two transport traits, plus a
//! branch in [`Programmer::open`] / `open_i2c` / `set_clock` — nothing
//! in `ops` / `spi` / `i2c` changes.

use crate::ch341::Ch341;
use crate::error::Result;
use crate::i2c::I2cTransport;
use crate::spi::SpiTransport;

/// A connected USB flash programmer. One variant per supported bridge.
pub enum Programmer {
    /// WCH CH341A — the original (and currently only) backend.
    Ch341(Ch341),
}

impl Programmer {
    /// Open the connected programmer in SPI mode.
    ///
    /// CH341A is currently the only backend, so this opens it directly.
    /// When a second bridge lands, this becomes a probe: try each
    /// known (VID, PID) signature in turn and return whichever is
    /// present, or [`Error::DeviceNotFound`](crate::error::Error) if
    /// none are.
    pub fn open(verbose: bool) -> Result<Self> {
        Ok(Self::Ch341(Ch341::open(verbose)?))
    }

    /// Open the connected programmer in I²C mode. See [`Programmer::open`].
    pub fn open_i2c(verbose: bool) -> Result<Self> {
        Ok(Self::Ch341(Ch341::open_i2c(verbose)?))
    }

    /// Set the bus clock in kHz. Each backend validates the value
    /// against the rates it actually supports.
    pub fn set_clock(&self, khz: u32) -> Result<()> {
        match self {
            Self::Ch341(dev) => dev.set_clock(khz),
        }
    }
}

impl SpiTransport for Programmer {
    fn spi_transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Ch341(dev) => dev.spi_transfer(tx),
        }
    }
}

impl I2cTransport for Programmer {
    fn i2c_transfer(&mut self, slave_7bit: u8, tx: &[u8], rx_len: usize) -> Result<Vec<u8>> {
        match self {
            Self::Ch341(dev) => dev.i2c_transfer(slave_7bit, tx, rx_len),
        }
    }

    fn i2c_probe(&mut self, slave_7bit: u8) -> Result<bool> {
        match self {
            Self::Ch341(dev) => dev.i2c_probe(slave_7bit),
        }
    }
}
