# CH341 Flash Programmer — Project Prompt

## Project Goal

Build a cross-platform CLI tool for programming SPI/I²C flash chips using the CH341A USB programmer. The tool should work on Linux, macOS (including Apple Silicon), and Windows without requiring kernel drivers.

---

## Project Name

Choose a name. Suggested: `ch3aser`, `flashwave`, or `pinflash` — or propose something better.

---

## Language & Stack

- **Language:** Rust (preferred) or C
- **USB backend:** `libusb-1.0` via the `rusb` crate (Rust) or `libusb` directly (C)
- **CLI parsing:** `clap` v4 (Rust) or `argp` (C)
- **Chip database:** Stored as a TOML or JSON file, loaded at runtime
- **Build system:** Cargo (Rust) or CMake (C)
- **Target platforms:** Linux x86_64/ARM64, macOS x86_64/ARM64 (Apple Silicon), Windows x86_64

---

## Repository Layout

```
/
├── CLAUDE.md               ← this file
├── Cargo.toml              ← (Rust) workspace manifest
├── src/
│   ├── main.rs             ← CLI entry point
│   ├── ch341.rs            ← CH341A USB protocol layer
│   ├── spi.rs              ← SPI flash operations
│   ├── i2c.rs              ← I²C EEPROM operations
│   ├── chipdb.rs           ← Chip database loader
│   └── ops.rs              ← High-level read/write/verify logic
├── chips/
│   └── chips.toml          ← Chip database (JEDEC IDs → profiles)
├── platform/
│   ├── udev/
│   │   └── 99-ch341a.rules ← Linux udev rules
│   └── windows/
│       └── ch341a.inf      ← Windows WinUSB inf file
└── README.md
```

---

## Core Modules to Implement

### 1. `ch341.rs` — USB/Hardware Layer

Implement the CH341A USB protocol over libusb. Key responsibilities:

- Open device by USB VID/PID: `0x1A86:0x5512`
- Claim interface 0
- Set SPI clock speed (supported rates: 400kHz, 750kHz, 1.5MHz, 3MHz, 6MHz, 12MHz, 24MHz)
- Implement raw SPI byte transfer (CS assert → bulk transfer → CS deassert)
- Implement I²C start/stop/byte read/write
- Handle device-not-found and permission errors with helpful messages

### 2. `spi.rs` — SPI Flash Protocol

Implement standard SPI NOR flash commands:

| Command | Opcode | Description |
|---------|--------|-------------|
| JEDEC ID | `0x9F` | Read manufacturer + device ID |
| Read Status | `0x05` | Read SR1 |
| Write Enable | `0x06` | Required before erase/write |
| Chip Erase | `0xC7` | Full chip erase |
| Sector Erase (4K) | `0x20` | Erase 4KB sector |
| Block Erase (64K) | `0xD8` | Erase 64KB block |
| Page Program | `0x02` | Write up to 256 bytes |
| Read Data | `0x03` | Read arbitrary length |

Implement busy-wait polling via Read Status Register (bit 0 = WIP).

### 3. `chipdb.rs` — Chip Database

Load `chips/chips.toml` at runtime. Each entry should contain:

```toml
[[chip]]
name = "W25Q128JV"
jedec_id = "EF4018"
size_kb = 16384
page_size = 256
sector_size = 4096
erase_time_ms = 200
notes = "Common BIOS flash"
```

On startup, read JEDEC ID and look up the chip automatically. Allow `--chip` flag to override with a manual chip name.

### 4. `ops.rs` — High-Level Operations

Implement these operations as discrete, testable functions:

- `read(start: u32, len: u32, output: &Path)` — dump flash region to file
- `erase_chip()` — full chip erase with progress
- `erase_range(start: u32, len: u32)` — erase only needed sectors
- `write(data: &[u8], start: u32, verify: bool)` — program with optional readback verify
- `verify(data: &[u8], start: u32)` — compare file to chip, return diff count
- `blank_check()` — confirm all bytes are 0xFF
- `detect()` — identify chip, print name/size/status registers

All operations with significant duration should emit progress via a simple callback or progress bar (use `indicatif` crate).

---

## CLI Interface

```
USAGE:
    flashtool [OPTIONS] <COMMAND>

COMMANDS:
    detect              Read JEDEC ID and display chip info
    read                Dump flash contents to file
    write               Program flash from file
    erase               Erase chip or address range
    verify              Compare file to chip without writing
    blank-check         Confirm chip is fully erased

OPTIONS:
    -c, --chip <NAME>       Override chip auto-detection
    -s, --speed <KHZ>       SPI clock speed [default: 1500]
    -v, --verbose           Print raw SPI transactions
    -n, --dry-run           Parse input and validate, no hardware access
    -h, --help              Print help
    -V, --version           Print version

READ OPTIONS:
    -o, --output <FILE>     Output file [default: flash_dump.bin]
    --start <ADDR>          Start address in hex [default: 0x0]
    --length <BYTES>        Number of bytes to read

WRITE OPTIONS:
    -i, --input <FILE>      Input binary file (required)
    --no-verify             Skip readback verify after write
    --no-erase              Skip erase before write (dangerous)
    --start <ADDR>          Start address [default: 0x0]

ERASE OPTIONS:
    --range <START:LEN>     Erase address range instead of full chip
```

---

## Error Handling

- Use `thiserror` for typed error enums
- Distinguish: `DeviceNotFound`, `PermissionDenied`, `ChipNotRecognized`, `VerifyFailed { addr, expected, actual }`, `WriteProtected`, `IoError`
- On `PermissionDenied` on Linux, print: `"Try: sudo cp platform/udev/99-ch341a.rules /etc/udev/rules.d/ && sudo udevadm control --reload"`
- On `DeviceNotFound` on Windows, print a note about Zadig/WinUSB

---

## Platform Notes

### Linux
- Install udev rule from `platform/udev/99-ch341a.rules`
- VID/PID: `1a86:5512`
- No kernel module needed — use libusb userspace

### macOS (including Apple Silicon / M-series)
- No driver needed, libusb works natively
- Homebrew install: `brew install libusb`
- Test on both x86_64 and ARM64 targets

### Windows
- User must run Zadig and bind WinUSB to the CH341A device
- Include `platform/windows/ch341a.inf` with correct VID/PID
- Ship a pre-built `.exe` in releases

---

## Testing Strategy

- **Unit tests:** Mock the USB layer with a trait so `spi.rs` and `ops.rs` can be tested without hardware
- **Integration tests:** Tag with `#[cfg(feature = "hardware")]` and skip in CI
- **Chip DB test:** Validate that `chips.toml` parses without error and all JEDEC IDs are unique

---

## Dependencies (Cargo.toml)

```toml
[dependencies]
rusb = "0.9"
clap = { version = "4", features = ["derive"] }
thiserror = "1"
indicatif = "0.17"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
hex = "0.4"
sha2 = "0.10"   # for hash output on read

[dev-dependencies]
mockall = "0.12"
```

---

## Initial Tasks for Claude Code

1. Scaffold the Cargo project with the layout above
2. Implement `ch341.rs` with device open/close and raw SPI transfer
3. Add `chips/chips.toml` with at least 20 common BIOS/EEPROM chips (W25Q series, MX25L series, GD25Q series, SST25VF series)
4. Implement `detect` command end-to-end (open device → JEDEC read → DB lookup → print result)
5. Implement `read` command
6. Implement `erase` + `write` + `verify` commands
7. Add progress bars to all long operations
8. Write unit tests for the chip DB loader and the verify diff logic
9. Write `README.md` with install instructions for all three platforms
10. Add a `Makefile` or `justfile` with targets: `build`, `test`, `release`, `lint`

For protocol details, open-source firmware-flashing tools that target the CH341A are a useful cross-reference for tested byte sequences and timing.
