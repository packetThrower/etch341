---
title: I²C EEPROM workflow
description: Scanning, reading, writing, and verifying 24Cxx I²C EEPROMs with etch341. Covers chip-selection, pin straps, and the bit-stuffing convention for 24C04/08/16.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/usage/i2c.md
---

:::caution
The I²C path is **not yet hardware-validated**. The protocol layer,
CH341A transport, CLI subcommands, and chip database are all in
place and pass 16 mock-transport unit tests — but nobody's run the
end-to-end workflow against a real 24Cxx chip yet. Your first run
is also our bring-up. If something's off, please open an issue
with the verbose-mode (`-v i2c scan`) output and we'll get it
sorted.
:::

## How it differs from SPI

The big one: **no JEDEC ID**. 24Cxx EEPROMs don't have a
manufacturer-ID register, so etch341 can't auto-detect which chip
you've connected. Every I²C command needs `--chip <NAME>`
explicitly. The [chip database](/etch341/reference/chips/) lists
all 10 supported families.

Otherwise the operations look familiar — `read`, `write`, `verify`,
`blank-check`, `erase` (which is really just "write 0xFF
everywhere" since I²C EEPROMs don't have a true erase op).

## 1. Bus scan

```sh
etch341 i2c scan
```

Probes every 7-bit address in `0x08..0x77` and lists which ones
respond. A standard 24Cxx with pin straps tied to ground will
ACK at `0x50`.

If you see addresses in the `0x60`+ range, that's typically VRM
controllers, sensors, or other I²C devices sharing the bus. The
EEPROM is the one at `0x50` (or one of `0x50..0x57` if the chip's
A0/A1/A2 pins are strapped).

If you see nothing, see [troubleshooting](#troubleshooting).

## 2. Pick the chip and read

```sh
etch341 -c 24C256 i2c read -o eeprom.bin
```

Pick the chip name that matches what's printed on the package:

- `24C01` → 128 B
- `24C02` → 256 B (most common — every monitor's EDID is one of these)
- `24C04` → 512 B
- `24C08` → 1 KB
- `24C16` → 2 KB
- `24C32` → 4 KB
- `24C64` → 8 KB
- `24C128` → 16 KB
- `24C256` → 32 KB
- `24C512` → 64 KB

The 24C04 / 08 / 16 use bit-stuffing in the slave address byte for
their high memory bits (because they predate 2-byte memory
addresses). etch341 handles this automatically — you pass `-c
24C16`, the protocol layer manages the slave-address rotation.

## 3. Verify before writing

```sh
etch341 -c 24C256 i2c verify -i candidate.bin
```

Returns 0 if every byte matches, 1 with a count + first 5
mismatches otherwise. Use this to confirm the chip's contents are
what you think they are before modifying.

## 4. Write

```sh
etch341 -c 24C256 i2c write -i new-data.bin
```

Writes page-by-page (page size varies per chip: 8 B for 24C01/02,
16 B for 24C04/08/16, 32 B for 24C32/64, 64 B for 24C128/256, 128 B
for 24C512), with ACK polling between pages to wait out the chip's
internal write cycle. Verification runs automatically after; add
`--no-verify` to skip.

## 5. Erase

I²C EEPROMs don't have a real erase opcode. `etch341 i2c erase`
is shorthand for "write `0xFF` to every byte" — slower than a SPI
chip-erase (every byte takes a normal write cycle) but produces
the same end state.

```sh
etch341 -c 24C02 i2c erase
etch341 -c 24C02 i2c blank-check    # confirm all 0xFF
```

## Pin straps

If the chip's A0 / A1 / A2 pins are wired to something other than
ground (e.g. on a board where multiple EEPROMs share an I²C bus
and need distinct addresses), pass the 3-bit strap value:

```sh
etch341 -c 24C02 --straps 0x03 i2c read -o ee.bin
```

A0/A1/A2 → bits 0/1/2 respectively. Strap `0` (default) means
all three pins tied to ground; the chip lives at slave address
`0x50`. `--straps 0x03` puts it at `0x53`. Run `i2c scan` first
if you're not sure.

## Troubleshooting

### `No I²C devices responded on 0x08..0x77`

Most likely:

1. **Clip wiring wrong** — see the
   [Wiring page](/etch341/usage/wiring/). The CH341A uses different
   pins for I²C (`D0`=SCL, `D1`=SDA) than for SPI; if you have
   the clip still wired for an SPI flash, signals don't reach the
   EEPROM.
2. **No power** — multimeter pin 8 to GND should read ~3.3V.
3. **Chip isn't an I²C EEPROM at all** — on a board, the SOIC-8
   you're clipped to might be a buck regulator or some other
   non-I²C-EEPROM part. The [Wiring page's "Is it really an EEPROM"
   section](/etch341/usage/wiring/#is-the-chip-really-an-i²c-eeprom)
   walks through how to tell.

### `etch341 i2c scan` returns every address

That's the opposite failure — the CH341A's I²C ACK-bit polarity
might be inverted relative to what the etch341 transport
assumes. This is the one documented hardware-validation gap; see
the warning at the top of this page. Open an issue with the
verbose output (`etch341 -v i2c scan`) and we'll get it
sorted.

### Write fails partway through

EEPROMs can be physically worn out after ~100,000-1,000,000 writes
per byte. If a long-used chip starts failing verify on writes that
worked yesterday, the chip itself may be at end-of-life. Try a
different one before chasing protocol issues.
