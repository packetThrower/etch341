---
title: CLI reference
description: Every etch341 subcommand and global flag, with examples for detect / read / write / erase / verify / blank-check / i2c / chips / strings / search.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/usage/cli.md
---

The same binary is the CLI. Any subcommand runs in CLI mode; no
subcommand opens the GUI. `etch341 --help` and `etch341 <command>
--help` are the canonical references — this page is a navigable
overview.

## Global flags

These apply to every subcommand that touches hardware:

| Flag | Meaning |
|---|---|
| `-c, --chip <NAME>` | Override JEDEC autodetect with a name from the [chip database](/etch341/reference/chips/) (e.g. `W25Q128JV`). Required for I²C ops (no JEDEC equivalent) and for `--dry-run`. |
| `-s, --speed <KHZ>` | Bus clock speed in kHz. Supported on the CH341A: `20`, `100`, `400`, `750`. Default `750`. Applies to both SPI and I²C. |
| `-v, --verbose` | Log every SPI or I²C transaction to stderr. Invaluable for debugging in-circuit issues; every `-> OUT` line should be followed by a sensible `<- IN`. |
| `-n, --dry-run` | Validate everything possible offline (chip name, range, input file readable) and print a `[dry-run]` summary of what *would* happen, without opening the CH341A. Hardware-touching commands only — offline commands (`chips`, `strings`, `search`) ignore the flag. |

## SPI flash commands

```sh
etch341 detect                              # JEDEC ID + chip lookup
etch341 read -o bios.bin                    # dump entire chip
etch341 read -o -                           # dump to stdout — pipe to anything
etch341 read -o - | sha256sum               # hash a chip without a temp file
etch341 read -o - | diff - bios.bin         # quick "did anything change"
etch341 read -o head.bin --length 0x1000    # first 4 KB only
etch341 read -o tail.bin --start 0x10000 --length 0x10000   # 64 KB block
etch341 write -i bios.bin                   # erase + write + verify
etch341 write -i bios.bin --no-erase --no-verify   # raw program (advanced)
etch341 erase                               # full chip erase
etch341 erase --range 0x10000:0x10000       # erase one 64 KB block
etch341 verify -i bios.bin                  # readback compare
etch341 blank-check                         # confirm all 0xFF
etch341 sr                                  # dump SR1/SR2/SR3 with decoded bits
```

Address parsing accepts decimal (`65536`) or `0x`-prefixed hex
(`0x10000`). `--range START:LEN` uses the same format on either
side of the colon.

For chips bigger than 16 MB, etch341 automatically switches to
4-byte addressing (opcodes `0x13` / `0x12` / `0x21` / `0xDC`) so the
operations work transparently up to the maximum 32-bit address
space.

## I²C EEPROM commands

Unlike SPI there's no JEDEC ID register, so `--chip <NAME>` is
mandatory:

```sh
etch341 i2c scan                            # probe 0x08..0x77, list ACKers
etch341 -c 24C256 i2c read -o eeprom.bin
etch341 -c 24C256 i2c write -i eeprom.bin
etch341 -c 24C256 i2c verify -i eeprom.bin
etch341 -c 24C02 i2c blank-check
etch341 -c 24C02 i2c erase                  # write 0xFF to every byte
```

The `--straps <0..7>` flag selects the A0/A1/A2 pin value if the
chip is wired non-default. The 24C04 / 24C08 / 24C16 use
bit-stuffing in the slave address byte for their high memory bits;
this is handled automatically.

:::caution
The I²C path is implemented + unit-tested against a mock transport,
but hasn't yet been validated against a real 24Cxx chip on
silicon. See the [I²C workflow](/etch341/usage/i2c/) page for the
caveats.
:::

## Offline / file-inspection commands

These don't touch the CH341A. They work on local files or the
embedded chip database.

```sh
etch341 chips                                  # list every supported chip
etch341 chips --find mx25                      # substring filter on name or JEDEC
etch341 chips --bus i2c                        # filter to one bus family

etch341 strings -i dump.bin                    # printable ASCII strings ≥4 chars
etch341 strings -i dump.bin --min-len 8        # higher threshold = less noise

etch341 search "55 AA" -i dump.bin             # find hex pattern
etch341 search "Award" -i dump.bin             # ASCII (case-insensitive)
etch341 search "DEADBEEF" -i dump.bin --context 32   # widen the gutter
```

`search` interprets the pattern as hex bytes when the condensed
form is even-length and all hex digits (so `55AA`, `55 AA`, and
`DE AD BE EF` all become byte sequences). Anything else is ASCII.
Matched bytes print in upper-case hex; surrounding context in
lower-case for visual contrast.

## `--dry-run` examples

```sh
$ etch341 -n detect
[dry-run] would open CH341A and read JEDEC ID at 750 kHz

$ etch341 -n -c MX25L12835F read -o foo.bin
[dry-run] would read 16777216 bytes (0x00000000..0x01000000) from MX25L12835F → foo.bin

$ etch341 -n -c W25Q128JV write -i bios.bin
[dry-run] would erase + write 16777216 bytes from bios.bin to W25Q128JV at 0x00000000 + verify

$ etch341 -n -c W25QQQ128 read -o foo.bin
Error: "--chip W25QQQ128: not in chip DB (try `etch341 chips --find W25QQQ128`)"
```
