//! `AppView`'s methods, split by concern across these submodules.
//! Each adds an `impl AppView` block (see the files); `mod.rs` keeps
//! the struct, constructor, shared infra, and the `Render` impl.

mod bios;
mod diff;
mod hex;
mod i2c;
mod otp;
mod settings;
mod spi;
