# etch341

Cross-platform CLI + GUI flash programmer for the **CH341A** USB SPI/I²C
interface. No kernel drivers required.

## Status

Working programmer for SPI NOR up to 32 MB+, on both 3.3V and 1.8V chips
(with a CH341A V1.7+ module for the 1.8V parts). Chips ≤ 16 MB use
standard 3-byte addressing; > 16 MB chips use the 4-byte opcode variants
(0x13 / 0x12 / 0x21 / 0xDC) automatically based on the chip's size. Round-trip validated
against a real Macronix MX25U4033E on an NVIDIA GTX 1060 — full
erase → blank-check → write → verify cycle landed byte-identical (SHA-256
match) to the original VBIOS.

| Feature | CLI | GUI |
| --- | --- | --- |
| Detect (JEDEC ID → chip lookup) | ✅ | ✅ |
| Read | ✅ | ✅ |
| Erase (full + range) | ✅ | ✅ (arm/confirm) |
| Write (with erase + verify) | ✅ | ✅ (arm/confirm + file picker) |
| Verify | ✅ | ✅ (file picker) |
| Blank check | ✅ | ✅ |
| Settings (clock speed, etc.) | ✅ (`--speed`) | ✅ |
| 4-byte addressing (>16 MB chips) | ✅ | ✅ |
| I²C scan / read / write / verify / blank-check / erase | ✅ | — |

52 unit tests covering the SPI / I²C protocols, the high-level ops,
and the inspect/search primitives, all running against mock transports
or pure inputs. Hardware-touching tests are gated behind
`--features hardware`.

### Hardware-validated

- **Macronix MX25U4033E** (1.8V, 4 Mbit) on a GTX 1060 VBIOS chip
  (GP106 PG410). Full erase → write → verify cycle returns the chip
  to a byte-identical state (matching SHA-256 across pre- and
  post-cycle reads).
- **CH341A V1.7** mini programmer with on-board ZIF socket + SOIC-8
  clip. The V1.7 has a 1.8V mode that older V1.3 boards lack — required
  for the U-series Macronix chips and most modern GPU VBIOS.

Other chips in `chips/chips.toml` are entered from datasheets but
haven't been individually exercised against silicon. If you run a
JEDEC `detect` on a chip and the response decodes correctly to a
named entry, the rest of the operations are very likely to work
(they're chip-agnostic at the protocol level).

## Install

### Prerequisites

- Rust 1.85+ (uses 2024 edition)
- libusb 1.0
- A CH341A USB programmer (the common "black module" or the "V1.3 mini" with
  on-board ZIF socket both work)

### macOS

```sh
brew install libusb
cargo install --path .
```

No driver setup needed — macOS leaves the CH341A's vendor interface alone.

### Linux

```sh
sudo apt install libusb-1.0-0-dev   # or your distro's equivalent
sudo cp platform/udev/99-ch341a.rules /etc/udev/rules.d/
sudo udevadm control --reload
cargo install --path .
```

The udev rule lets unprivileged users open the device. Without it you'll
hit `PermissionDenied`.

### Windows

Windows doesn't ship a generic userspace USB driver, so the CH341A
either enumerates as an unknown device or gets claimed by a vendor
serial-port driver — either way, libusb can't open it. The one-time
fix is to bind the **WinUSB** generic driver to the device:

1. Plug in the CH341A.
2. Run [Zadig](https://zadig.akeo.ie/) (≈600 KB, no installer).
3. In Zadig's `Options` menu, enable `List All Devices`.
4. Select the entry with VID `0x1A86` / PID `0x5512`, choose **WinUSB**
   from the driver dropdown, and click `Install Driver`.
5. `cargo install --path .`

You only need to do steps 1–4 once per machine. If `etch341 detect`
reports `DeviceNotFound` on Windows after running it once, the driver
binding is usually the cause — re-check in Zadig that the device is
still bound to WinUSB and not to a vendor driver that took over after
an update.

## Usage

### CLI

```sh
etch341 detect                       # identify the chip
etch341 read -o bios.bin             # dump entire chip to file
etch341 read -o head.bin --length 0x1000   # first 4 KB only
etch341 write -i bios.bin            # erase + program + verify
etch341 write -i bios.bin --no-erase --no-verify   # raw program
etch341 erase                        # full chip erase
etch341 erase --range 0x10000:0x10000   # erase one 64 KB block
etch341 verify -i bios.bin           # compare without writing
etch341 blank-check                  # confirm all 0xFF
```

I²C EEPROMs (24Cxx family) use the nested `i2c` subcommand. Unlike
SPI NOR there's no JEDEC ID register, so the chip must be selected
explicitly with `-c`:

```sh
etch341 i2c scan                            # list 7-bit addrs that ACK
etch341 -c 24C256 i2c read -o eeprom.bin    # dump entire chip
etch341 -c 24C256 i2c write -i eeprom.bin   # program + verify
etch341 -c 24C256 i2c verify -i eeprom.bin  # compare without writing
etch341 -c 24C02 i2c blank-check            # confirm all 0xFF
etch341 -c 24C02 i2c erase                  # write 0xFF to every byte
```

`--straps <0..7>` selects the A0/A1/A2 pin value if the chip is wired
non-default. The 24C04/08/16 use bit-stuffing in the slave address
for their high memory bits; this is handled automatically.

Supported families: 24C01 / 02 / 04 / 08 / 16 / 32 / 64 / 128 / 256 /
512. Other 24Cxx chips work if you add an entry to `chips/i2c_chips.toml`.
```

The CLI also has three offline inspection commands that work on flash
dump files (no hardware required):

```sh
etch341 chips                            # list every supported chip
etch341 chips --find mx25                # substring filter on name or JEDEC
etch341 chips --bus i2c                  # filter to one bus family

etch341 strings -i dump.bin              # printable ASCII strings ≥4 chars
etch341 strings -i dump.bin --min-len 8  # noisier-but-richer threshold

etch341 search "55 AA" -i dump.bin       # find hex pattern (spaces optional)
etch341 search "Award" -i dump.bin       # ASCII (case-insensitive)
etch341 search "DEADBEEF" -i dump.bin --context 32   # widen the gutter
```

`search` parses the pattern as hex when the condensed form is even-length
and all hex digits (`55AA`, `DE AD BE EF`); anything else is taken as
ASCII. Matched bytes print in upper-case hex; surrounding context stays
lower-case for an at-a-glance visual contrast.

Global flags:

- `-v, --verbose` — log every SPI or I²C transaction to stderr.
  Invaluable for debugging in-circuit issues and for spotting wiring
  problems (every `-> OUT` line should be followed by a sensible
  `<- IN`; missing IN bytes mean either the chip isn't responding or
  the bus is mis-wired).
- `-c, --chip <NAME>` — for SPI, overrides JEDEC autodetect with a
  chip name from `chips/chips.toml` (e.g. `W25Q128JV`). For I²C and
  for `--dry-run` it's **required** (there's no JEDEC equivalent on
  I²C, and dry-run has no hardware to autodetect).
- `-s, --speed <KHZ>` — bus clock speed for both SPI and I²C.
  Supported rates on the CH341A: 20, 100, 400, 750 (default 750).
- `-n, --dry-run` — for hardware-touching commands, validate
  everything possible (chip name in DB, input file is readable,
  start + length fits the chip) and print a `[dry-run]` summary of
  what would happen. Never opens the CH341. Useful for sanity-
  checking flags before you actually pull the trigger on an erase or
  write. Offline commands (`chips`, `strings`, `search`) ignore the
  flag because they don't touch hardware anyway.

### GUI

```sh
etch341      # no subcommand → opens the GUI window
```

Build the CLI-only variant (no GPUI fetch, much smaller binary, faster
build) with:

```sh
cargo build --release --no-default-features
```

## Hardware notes

### In-circuit programming on enterprise hardware

In-circuit attempts on **server-class boards, dual-BIOS systems, and
firewalls** frequently fail. The host's SPI controller actively drives MISO
low even when the board is "powered off" — `etch341 detect` returns
`JEDEC ID : 0x000000` and the verbose log shows clean command bytes going
out but nothing meaningful coming back.

Diagnose with the loopback test:

```sh
etch341 detect -v       # with clip OFF the chip; nothing else changed
```

- `<- IN [4]: ffffffff` → CH341A is healthy; the target board is fighting us
- `<- IN [4]: 00000000` → CH341A or wiring problem, not the target

Remedies, in order of effort:

1. Use a loose chip in the CH341A's on-board ZIF socket
2. Lift pin 8 (VCC) of the in-circuit chip and inject 3.3V externally
3. Hot-air the chip off and use the ZIF

### Voltage

The black-module CH341A has a 3.3V/5V jumper near the USB end. **3.3V is
correct for every chip currently in `chips/chips.toml`** (W25Q, MX25L,
GD25Q, AT25SF). 5V will damage 3.3V-only parts.

### Pin 1

The SOIC-8 clip's red wire = pin 1. The chip's pin 1 is marked with a dot
or notch on the package. About half of first-attempt failures are
clip-reversed.

## Architecture

```
src/
├── main.rs       entry point; no-args → GUI, subcommand → CLI
├── cli.rs        clap derive definitions + dispatch
├── error.rs      thiserror enum
├── ch341.rs      USB layer; impls both SpiTransport and I2cTransport
├── spi.rs        SPI NOR opcodes + SpiTransport trait + helpers
├── ops.rs        high-level SPI read / erase / write / verify / blank / detect
├── i2c.rs        24Cxx protocol + I2cTransport trait + helpers
├── i2c_ops.rs    high-level I²C scan / read / write / verify / blank / erase
├── chipdb.rs     TOML chip DB loader (SPI + I²C, embedded at build)
├── inspect.rs    parse-pattern / extract-strings / find-pattern shared by CLI + GUI
├── prefs.rs      ~/.config/etch341/prefs.toml load/save (GUI settings)
└── gui/          GPUI frontend; behind the `gui` cargo feature (default-on)

chips/chips.toml      24 SPI NOR entries (W25Q, MX25L, GD25Q, SST25VF, AT25SF)
chips/i2c_chips.toml  10 I²C EEPROM entries (24C01 .. 24C512)
```

The `SpiTransport` trait abstracts the USB layer so the high-level ops can
be unit-tested against a deterministic mock (`src/spi.rs::test_support::MockSpi`).
The `Ch341` struct is the production implementation.

## Development

```sh
just build        # full build (CLI + GUI; first time pulls the gpui git dep)
just build-cli    # CLI only, much faster
just test         # unit tests, no hardware
just run -- detect -v
```

Or use Cargo directly:

```sh
cargo build --no-default-features    # CLI only
cargo test  --no-default-features
cargo run                            # GUI
cargo run   --no-default-features -- detect -v
```

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) for the full text.
