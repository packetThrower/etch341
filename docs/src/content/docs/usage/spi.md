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

### Pipe-friendly dumps

`-o -` writes the chip's bytes to stdout instead of a file —
useful for hashing or diffing without a temp file in between:

```sh
etch341 read -o - | sha256sum             # fingerprint a chip
etch341 read -o - | diff - bios.bin       # has it changed?
```

The "Read OK / SHA-256" summary lines are suppressed in stdout
mode so they don't interleave with the binary data on the
consumer side. Errors still surface on stderr.

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

## 7. Status registers

```sh
etch341 sr
```

Reads the three SPI status registers (SR1 / SR2 / SR3) and decodes
the standard bit fields. The two most common reasons you'd reach
for this:

- **Writes silently fail** — the chip is happily ACK-ing the write
  but the data never lands. Almost always block-protect bits
  (`BP[2:0]`, `SEC/BP3`) set in SR1. Clear them via WRSR before
  programming. `etch341 sr` will surface the gotcha with a
  follow-up note.
- **Quad-mode opcodes NACK** — `QE` (Quad Enable) is clear in SR2.
  Mostly only matters if you're trying to use the chip in QSPI
  mode; standard `0x03` / `0x02` reads + writes work either way.

### What's universal vs vendor-specific

`SR1` (read via opcode `0x05`) is read by **every** SPI NOR chip,
and the decoded bits (`WIP` / `WEL` / `BP[2:0]` / `TB` / `SRP0`)
mean the same thing on Winbond, Macronix, GigaDevice, ISSI, EON,
etc. The one wrinkle is bit 6 — Winbond calls it `SEC` (4 KB vs
64 KB protect granularity), Macronix calls it `BP3` (extending the
block-protect mask by one bit). Same position, different semantic;
`etch341 sr` labels it `SEC/BP3` to hedge.

`SR2` (opcode `0x35`) is on most chips made since ~2012 — W25Q,
modern MX25L / MX25U, GD25Q. Older or simpler parts (some
SST25VF, EN25, basic AT25) don't implement it; `etch341 sr` shows
`0xFF (chip didn't respond — likely doesn't implement SR2)`
rather than decoding garbage.

`SR3` (opcode `0x15`) is the most vendor-specific — pure W25Q
convention. On a Macronix or older chip you'll see the same
"didn't respond" fallback.

The decoded labels for SR2 / SR3 follow the W25Q-family convention.
Raw hex + binary are always shown so you can cross-check against
the chip's datasheet if the labels don't apply.

### Example output

```text
$ etch341 sr
JEDEC ID : 0xEF4018
Chip     : W25Q128JV

SR1 : 0x00  (0b00000000)
        WIP=0 WEL=0 BP=0 TB=0 SEC/BP3=0 SRP0=0
SR2 : 0x02  (0b00000010)
        SRP1=0 QE=1 LB=0 CMP=0 SUS=0
SR3 : 0x60  (0b01100000)
        ADP=0 WPS=0 DRV=3 HOLD/RST=0
```

A chip without SR2 / SR3 (like a Macronix MX25U4033E pulled from a
graphics card):

```text
$ etch341 sr
JEDEC ID : 0xC22533
Chip     : MX25U4033E

SR1 : 0x00  (0b00000000)
        WIP=0 WEL=0 BP=0 TB=0 SEC/BP3=0 SRP0=0
SR2 : 0xFF    (chip didn't respond — likely doesn't implement SR2)
SR3 : 0xFF    (chip didn't respond — likely doesn't implement SR3)
```

The GUI's **Status regs** pane (under the workflow divider in
the sidebar) renders the same view — handy for keeping an eye on
SR1 across multiple chips without flipping back to the terminal.

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
