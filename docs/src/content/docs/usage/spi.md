---
title: SPI flash workflow
description: End-to-end SPI NOR programming with etch341 — detect, back up, erase, write, and verify a chip. Covers 3.3V vs 1.8V chips, 4-byte addressing for >16 MB chips, and in-circuit caveats.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/usage/spi.md
---

The canonical workflow for an SPI NOR chip — whether it's a
desktop BIOS, a GPU VBIOS, a router firmware, or just a loose part
on the bench — is **detect → back up → operate → verify**. Never
write to a chip you haven't backed up first.

## 0. Hook up the chip

Either insert it into the CH341A's on-board ZIF socket (loose
chip) or clip a SOIC-8 clip onto the chip in-circuit. The
[Wiring + voltage](/etch341/usage/wiring/) page covers pin
orientation, the 3.3V vs 1.8V jumper, and what to do when an
in-circuit host fights the programmer.

## 1. Detect

```sh
etch341 detect
```

Output:

```text
JEDEC ID : 0xEF4018
Chip     : W25Q128JV
Size     : 16384 KB (65536 pages of 256 B, 4096 sectors of 4096 B)
Notes    : Winbond 128 Mbit — common BIOS flash
```

If the JEDEC reads as all-zeros or all-ones, something's wrong —
see the [troubleshooting section](#troubleshooting) below before
proceeding.

If the chip's JEDEC isn't in etch341's database, `detect` reports
the bytes anyway and prints `unknown — pass --chip <NAME> to
override`. You can either add an entry to `chips/chips.toml` (see
[Chip database](/etch341/reference/chips/)) or override the chip
identity for one-off operations: `etch341 --chip W25Q128JV ...`

## 2. Back up the original contents

Before anything destructive:

```sh
etch341 read -o original-$(date +%Y%m%d).bin
```

This dumps the full chip to a timestamped file. The CLI also prints
the file's SHA-256 — keep that hash; you'll want it if you need to
confirm a future "restore" was identical to the original.

For sanity, do this **twice** and confirm both dumps match
byte-for-byte (`sha256sum`). A bad first read on a flaky clip is the
most common silent failure mode, and you want to catch it before
you've also erased the chip.

## 3. Write the new image

```sh
etch341 write -i new-firmware.bin
```

`write` runs **erase → program → verify** by default. The
verification at the end reads back every byte and compares it to
the input file; if any byte differs, the operation fails loudly with
the failing addresses logged.

If verify fails:

- **Mostly 0xFF** at the end: chip is smaller than the input file
  (you tried to write 32 MB to a 16 MB chip). Check `detect`'s
  capacity output.
- **Random differences**: bad write — could be a flaky clip, write
  protection (chip's WP# pin tied high by the board), or a bus
  speed problem. Try `--speed 100` to slow down to 100 kHz.

Advanced flags (rarely useful):

| | |
|---|---|
| `--no-erase` | Skip the erase step. Only safe if you're writing zeros over existing ones — flash can only flip `1 → 0` without an erase. |
| `--no-verify` | Skip readback. Saves time on huge chips but you've lost the safety net. |
| `--start <ADDR>` | Write at a specific offset instead of `0x0`. |

## 4. Erase a range (no write)

```sh
etch341 erase                              # full chip
etch341 erase --range 0x10000:0x10000      # 64 KB block starting at 0x10000
```

`--range` is `START:LEN`. The erase op picks the largest erase
opcode that fits (4 KB sector, 32 KB block, 64 KB block, or full
chip-erase) for the requested range.

## 5. Verify without writing

If you have a known-good binary and want to confirm the chip's
current contents match:

```sh
etch341 verify -i good-firmware.bin
```

Returns 0 if everything matches, 1 with a count of differing bytes
otherwise. The first 5 mismatches are logged with their addresses.

## 6. Blank check

After an erase, confirm the chip is fully `0xFF`:

```sh
etch341 blank-check
```

This is cheaper than reading the whole chip — it short-circuits at
the first non-`0xFF` byte and reports the offset.

## Troubleshooting

### `JEDEC ID : 0xFFFFFF` (MISO floats high)

Chip isn't responding. In order of likelihood:

1. **Clip orientation.** Pin 1 of the clip's red wire must align
   with the dot/notch on the chip. Half of first-try fails are
   clip-reversed.
2. **VCC not reaching the chip.** Multimeter pin 8 to GND should
   read ~3.3V (or ~1.8V for U-series Macronix and W25Q\*JW). If
   it's 0V, the clip's not seated, or the in-circuit host's power
   path is blocking it.
3. **HOLD#/CE# pulled low** by an in-circuit host. The chip's
   internal pull-up on pin 7 keeps HOLD# high when nothing
   external is connected, but a host MCU can hold it down to keep
   the chip in standby. See
   [in-circuit programming](/etch341/usage/wiring/#in-circuit-programming).

### `JEDEC ID : 0x000000` (MISO stuck low)

In-circuit host is actively driving MISO low. Same recovery as
above — usually means lifting pin 8 (VCC) of the chip to fully
isolate it from the host's SPI controller. See the Wiring page.

### Verify fails on a fresh write

Try `--speed 100` to drop the bus clock from 750 kHz to 100 kHz.
Long clip wires, in-circuit programming through a noisy host, and
flaky clip pins all benefit from slower clocking.
