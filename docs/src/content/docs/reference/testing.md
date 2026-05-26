---
title: Testing
description: How etch341's test suite is organised — mock-transport unit tests for the protocol layer, the hardware-validation gates, and what's been exercised against silicon vs. only on mocks.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/reference/testing.md
---

etch341 has 52 unit tests covering the SPI / I²C protocols, the
high-level ops, the chip database, and the inspect/search
primitives. Run them with:

```sh
cargo test --no-default-features --bin etch341
```

`--no-default-features` skips the GUI graph; the tests are
domain-layer (protocol, ops, chipdb, inspect) and don't need GPUI.
The full default-features `cargo test` also works but takes
substantially longer because of the gpui compile.

## How the tests are structured

### Mock-transport pattern

The SPI and I²C transports are abstracted behind traits:

```rust
pub trait SpiTransport {
    fn spi_transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>>;
}

pub trait I2cTransport {
    fn i2c_transfer(&mut self, slave_7bit: u8, tx: &[u8], rx_len: usize) -> Result<Vec<u8>>;
    fn i2c_probe(&mut self, slave_7bit: u8) -> Result<bool>;
}
```

The `Ch341` struct is the production implementation; `MockSpi` and
`MockI2c` are deterministic mocks. Each test records the exact byte
sequences it expects to see on the bus, plays back canned responses
for reads, and asserts at the end that every recorded interaction
was actually consumed.

This means every protocol-layer test is exact-byte-stream-verified.
A regression in (say) the 4-byte-address opcode sequencing surfaces
as an exact mismatch with the recorded `tx` bytes, with a hex diff
in the failure output.

### What's covered

**SPI** (`src/spi.rs`, `src/ops.rs`):

- JEDEC ID read, returns the 3 ID bytes
- Status register reads, WIP-bit polling for write-busy detection
- 3-byte vs 4-byte addressing op selection by chip size
- Sector erase (4K) on both addressing widths
- Block erase (64K) on both addressing widths
- Chip erase (`0xC7`)
- Page program (256 B), rejects boundary-crossing writes
- Read with arbitrary length, header stripping
- Wait-for-ready polling timeout
- High-level read / verify (incl. mismatch counting) /
  blank-check / erase-range / write (with erase + verify)

**I²C** (`src/i2c.rs`, `src/i2c_ops.rs`):

- Slave-address composition for 24Cxx (incl. the bit-stuffing
  convention for 24C04/08/16)
- 1-byte vs 2-byte memory-address encoding
- Read splitting at the 31-byte CH341 chunk boundary
- Write splitting at both page boundaries and the CH341 packet
  limit
- ACK-polling for the wait-for-ready handshake after a page write
- Range validation (read/write past chip end rejected)
- High-level verify (mismatch counting) and blank-check

**Inspect / search** (`src/inspect.rs`):

- Hex/ASCII pattern auto-detection (`55 AA` → bytes, `Award` →
  string)
- String extraction with min-length filter, including trailing-run
  handling
- Pattern search with case-insensitive ASCII matching, binary
  exact-match, overlapping-match enumeration

**Chip database** (`src/chipdb.rs`):

- The bundled TOML files parse without error
- Every SPI JEDEC ID is unique
- Every SPI JEDEC ID is exactly 6 hex chars
- Case-insensitive lookup works for both JEDEC and chip names

## What hasn't been hardware-tested yet

Mock tests catch protocol-layer regressions but can't catch
hardware-protocol mismatches. Several paths are exercised only on
the mock so far:

| Path | Mock-tested | Silicon-tested |
|---|---|---|
| SPI detect + read + erase + write + verify (3-byte) | ✅ | ✅ (MX25U4033E on GTX 1060) |
| SPI 4-byte addressing (>16 MB chips) | ✅ | ❌ |
| I²C protocol layer | ✅ | ❌ |
| CH341A I²C ACK-bit polarity | ❌ (assumption) | ❌ |
| 24C04/08/16 bit-stuffing on the wire | ✅ | ❌ |

The CH341A ACK-bit polarity assumption in particular is the most
likely source of an I²C-path failure on first hardware contact. If
the polarity is inverted from what the transport assumes, `i2c
scan` either returns every address or none. See the
[I²C workflow troubleshooting](/etch341/usage/i2c/#troubleshooting).

## Hardware-validation tests

There's a Cargo feature flag `hardware` reserved for tests that
need a real CH341A + chip attached:

```sh
cargo test --features hardware
```

No tests are tagged with this feature yet — the gate exists so
future integration tests can be added without breaking CI on
runners that don't have a CH341A attached.

## CI

[`.github/workflows/ci.yml`](https://github.com/packetThrower/etch341/blob/main/.github/workflows/ci.yml)
runs `cargo fmt --check`, `cargo build --no-default-features`,
`cargo clippy --all-targets -- -D warnings`, and `cargo test` on
every push and PR. The matrix covers all five GitHub-hosted native
runners: macOS arm64, Windows amd64 + arm64, Ubuntu amd64 + arm64.

A green CI run is a prerequisite for the release workflow — the
release-build matrix won't fire if any of the CI matrix entries
fail.
