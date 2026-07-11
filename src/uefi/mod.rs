//! Read-only UEFI image parsing: firmware volumes → FFS files →
//! sections (with decompression) → later: HII/IFR and NVRAM stores.
//!
//! Crate discipline: this module is destined to be extracted into a
//! standalone MIT-licensed crate, so nothing in `uefi::` may import
//! from the rest of etch341 (no `crate::error`, no `crate::gui`) and
//! everything operates on plain `&[u8]`.

// Consumed by the `bios settings` CLI/GUI surface in a later phase;
// until then the walker is only exercised by its tests.
#![allow(dead_code)]

pub mod fv;
