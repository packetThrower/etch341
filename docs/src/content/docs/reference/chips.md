---
title: Chip database
description: Every SPI NOR flash and I²C EEPROM family etch341 recognizes by JEDEC ID or chip name. Inventory plus how to add a new entry.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/reference/chips.md
---

etch341's chip database lives in two TOML files at the repo root,
embedded into the binary at build time via `include_str!`. Running
`etch341 chips` from the CLI lists the live, in-binary set.

## SPI NOR (116 entries)

Source: `chips/chips.toml`. Listed `jedec_id` is the 3-byte
response to opcode `0x9F`, formatted as 6 uppercase hex chars.

### Winbond W25Q (3.3V, manufacturer 0xEF, family 0x40)

| Chip | JEDEC | Size | Notes |
|---|---|---|---|
| W25Q80DV | EF4014 | 1 MB | 8 Mbit |
| W25Q16JV | EF4015 | 2 MB | 16 Mbit |
| W25Q32JV | EF4016 | 4 MB | 32 Mbit |
| W25Q64JV | EF4017 | 8 MB | 64 Mbit — common BIOS flash |
| W25Q128JV | EF4018 | 16 MB | 128 Mbit — common BIOS flash |
| W25Q256JV | EF4019 | 32 MB | 256 Mbit (4-byte addressing) |
| W25Q512JV | EF4020 | 64 MB | 512 Mbit, 3.3V (4-byte addressing) |

### Winbond W25X (legacy 3.3V, manufacturer 0xEF, family 0x30)

| Chip | JEDEC | Size |
|---|---|---|
| W25X10 | EF3011 | 128 KB |
| W25X20 | EF3012 | 256 KB |
| W25X40 | EF3013 | 512 KB |
| W25X80 | EF3014 | 1 MB |
| W25X16 | EF3015 | 2 MB |
| W25X32 | EF3016 | 4 MB |
| W25X64 | EF3017 | 8 MB |

### Winbond W25Q\*JW (1.8V, manufacturer 0xEF, family 0x60)

Common in newer Intel + AMD BIOS chips and M.2 modules. Needs a
1.8V-capable programmer — see [Wiring + voltage](/etch341/usage/wiring/#voltage).

| Chip | JEDEC | Size |
|---|---|---|
| W25Q16JW | EF6015 | 2 MB |
| W25Q32JW | EF6016 | 4 MB |
| W25Q64JW | EF6017 | 8 MB |
| W25Q128JW | EF6018 | 16 MB |
| W25Q256JW | EF6019 | 32 MB |

### Macronix MX25L (3.3V, manufacturer 0xC2)

| Chip | JEDEC | Size |
|---|---|---|
| MX25L1006E | C22011 | 128 KB |
| MX25L2006E | C22012 | 256 KB |
| MX25L4006E | C22013 | 512 KB |
| MX25L8005 | C22014 | 1 MB |
| MX25L1606E | C22015 | 2 MB |
| MX25L3206E | C22016 | 4 MB |
| MX25L6406E | C22017 | 8 MB |
| MX25L12835F | C22018 | 16 MB |
| MX25L25635F | C22019 | 32 MB |

### Macronix MX25U (1.8V, manufacturer 0xC2, family 0x25)

| Chip | JEDEC | Size | Notes |
|---|---|---|---|
| MX25U4033E | C22533 | 512 KB | Common GPU VBIOS (e.g. GTX 1060) — **hardware-validated** |
| MX25U8035E | C22534 | 1 MB | |
| MX25U1635F | C22535 | 2 MB | |
| MX25U3235F | C22536 | 4 MB | |
| MX25U6435F | C22537 | 8 MB | |
| MX25U12835F | C22538 | 16 MB | |
| MX25U25645G | C22539 | 32 MB | (4-byte addressing) |

### GigaDevice GD25Q + GD25LQ (manufacturer 0xC8)

| Chip | JEDEC | Size | Voltage |
|---|---|---|---|
| GD25Q80 | C84014 | 1 MB | 3.3V |
| GD25Q16 | C84015 | 2 MB | 3.3V |
| GD25Q32 | C84016 | 4 MB | 3.3V |
| GD25Q64 | C84017 | 8 MB | 3.3V |
| GD25Q128 | C84018 | 16 MB | 3.3V |
| GD25Q256 | C84019 | 32 MB | 3.3V (4-byte addressing) |
| GD25LQ16 | C86015 | 2 MB | 1.8V |
| GD25LQ32 | C86016 | 4 MB | 1.8V |
| GD25LQ64 | C86017 | 8 MB | 1.8V |
| GD25LQ128 | C86018 | 16 MB | 1.8V |

### Adesto / Atmel AT25 (manufacturer 0x1F)

Adesto's AT25 line spans three voltage classes: the **SF** and
**DF** families are 3.3V, the **SL** family is 1.8V (needs a
1.8V-capable programmer), and the low-voltage **DN** part runs
from 2.3V. Only AT25DN512C is silicon-confirmed; the rest carry
JEDEC IDs cross-referenced from published vendor ID tables.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| AT25SF041 | 1F8401 | 512 KB | 3.3V | |
| AT25SF081 | 1F8501 | 1 MB | 3.3V | |
| AT25SF161 | 1F8601 | 2 MB | 3.3V | |
| AT25SF321 | 1F8701 | 4 MB | 3.3V | |
| AT25DF041A | 1F4401 | 512 KB | 3.3V | |
| AT25DF081A | 1F4501 | 1 MB | 3.3V | also sold as AT26DF081A |
| AT25DF161 | 1F4602 | 2 MB | 3.3V | |
| AT25DF321A | 1F4701 | 4 MB | 3.3V | very common on routers / embedded |
| AT25DF641 | 1F4800 | 8 MB | 3.3V | 641 and 641A share this ID |
| AT25SL321 | 1F4216 | 4 MB | 1.8V | |
| AT25SL128A | 1F4218 | 16 MB | 1.8V | |
| AT25DN512C | 1F6501 | 64 KB | 2.3V | no SFDP; silicon-confirmed |

### Other 3.3V families

| Chip | JEDEC | Size | Manufacturer |
|---|---|---|---|
| SST25VF016B | BF2541 | 2 MB | SST (AAI word-program) |
| SST25VF032B | BF254A | 4 MB | SST |
| SST25VF064C | BF254B | 8 MB | SST |
| EN25QH32 | 1C7016 | 4 MB | EON |
| EN25QH64 | 1C7017 | 8 MB | EON |
| EN25QH128 | 1C7018 | 16 MB | EON |
| P25Q40H | 856013 | 512 KB | PUYA |
| P25Q80H | 856014 | 1 MB | PUYA |
| P25Q16H | 856015 | 2 MB | PUYA |
| P25Q32H | 856016 | 4 MB | PUYA |
| P25Q64H | 856017 | 8 MB | PUYA |
| IS25LP064 | 9D6017 | 8 MB | ISSI (FPGA dev boards) |
| IS25LP128 | 9D6018 | 16 MB | ISSI (FPGA dev boards) |

The families below were cross-referenced from published vendor ID
tables and are flagged not-yet-silicon-tested in the catalogue notes
until someone runs one. Parts ≥ 256 Mbit use 4-byte addressing (itself
not yet silicon-validated).

### Spansion / Cypress / Infineon S25FL (manufacturer 0x01)

Extremely common in networking, industrial, and automotive gear.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| S25FL116K | 014015 | 2 MB | 3.3V | |
| S25FL132K | 014016 | 4 MB | 3.3V | |
| S25FL164K | 014017 | 8 MB | 3.3V | |
| S25FL064L | 016017 | 8 MB | 3.3V | |
| S25FL128L | 016018 | 16 MB | 3.3V | |
| S25FL256L | 016019 | 32 MB | 3.3V | 4-byte addressing |
| S25FL128S | 012018 | 16 MB | 3.3V | also S25FL127S / 129P |
| S25FL256S | 010219 | 32 MB | 3.3V | 4-byte addressing |
| S25FL512S | 010220 | 64 MB | 3.3V | 4-byte addressing |

### Micron / Numonyx N25Q + MT25Q (manufacturer 0x20)

FPGA configuration flash and networking equipment. The 3.0V (BA) parts
kept the N25Q name through the MT25QL rename — same JEDEC ID.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| N25Q032 | 20BA16 | 4 MB | 3.0V | |
| N25Q064 | 20BA17 | 8 MB | 3.0V | |
| MT25QL128 | 20BA18 | 16 MB | 3.0V | same ID as N25Q128 |
| MT25QL256 | 20BA19 | 32 MB | 3.0V | also N25Q256; 4-byte |
| MT25QL512 | 20BA20 | 64 MB | 3.0V | also N25Q512; 4-byte |
| MT25QL01G | 20BA21 | 128 MB | 3.0V | 4-byte addressing |
| MT25QU128 | 20BB18 | 16 MB | 1.8V | |
| MT25QU256 | 20BB19 | 32 MB | 1.8V | 4-byte addressing |
| MT25QU512 | 20BB20 | 64 MB | 1.8V | 4-byte addressing |

### Microchip / SST SST26VF (manufacturer 0xBF, family 0x26)

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| SST26VF016B | BF2641 | 2 MB | 3.3V | |
| SST26VF032B | BF2642 | 4 MB | 3.3V | |
| SST26VF064B | BF2643 | 8 MB | 3.3V | |

### ISSI IS25LP / IS25WP (manufacturer 0x9D)

IS25WP is the 1.8V sibling of the 3.3V IS25LP line.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| IS25LP032 | 9D6016 | 4 MB | 3.3V | |
| IS25LP256 | 9D6019 | 32 MB | 3.3V | 4-byte addressing |
| IS25WP064 | 9D7017 | 8 MB | 1.8V | |
| IS25WP128 | 9D7018 | 16 MB | 1.8V | |
| IS25WP256 | 9D7019 | 32 MB | 1.8V | 4-byte addressing |

### XTX XT25F (manufacturer 0x0B, family 0x40)

Ubiquitous in cheap WiFi modules and ESP8266 / ESP32 boards.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| XT25F16B | 0B4015 | 2 MB | 3.3V | |
| XT25F32B | 0B4016 | 4 MB | 3.3V | |
| XT25F64B | 0B4017 | 8 MB | 3.3V | |
| XT25F128B | 0B4018 | 16 MB | 3.3V | |

### Zbit ZB25VQ (manufacturer 0x0E, family 0x40)

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| ZB25VQ16 | 0E4015 | 2 MB | 3.3V | |
| ZB25VQ32 | 0E4016 | 4 MB | 3.3V | |
| ZB25VQ64 | 0E4017 | 8 MB | 3.3V | |
| ZB25VQ128 | 0E4018 | 16 MB | 3.3V | |

### Boya / BoHong BY25Q (manufacturer 0x68, family 0x40)

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| BY25Q64AS | 684017 | 8 MB | 3.3V | |
| BY25Q128AS | 684018 | 16 MB | 3.3V | |

### Macronix MX25R (low-power) + large parts (manufacturer 0xC2)

MX25R is a wide-Vcc low-power line common in BLE / IoT designs.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| MX25R8035F | C22814 | 1 MB | 1.65–3.6V | low-power |
| MX25R1635F | C22815 | 2 MB | 1.65–3.6V | low-power |
| MX25R3235F | C22816 | 4 MB | 1.65–3.6V | low-power |
| MX25R6435F | C22817 | 8 MB | 1.65–3.6V | low-power |
| MX25L51245G | C2201A | 64 MB | 3.3V | also MX66L51235F; 4-byte |
| MX66L1G45G | C2201B | 128 MB | 3.3V | 4-byte addressing |

### EON EN25Q (manufacturer 0x1C, family 0x30)

The Q line; complements the EN25QH parts above.

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| EN25Q32 | 1C3016 | 4 MB | 3.3V | |
| EN25Q64 | 1C3017 | 8 MB | 3.3V | |
| EN25Q128 | 1C3018 | 16 MB | 3.3V | |

### GigaDevice GD25LQ (1.8V, manufacturer 0xC8, family 0x60)

| Chip | JEDEC | Size | Voltage | Notes |
|---|---|---|---|---|
| GD25LQ256 | C86019 | 32 MB | 1.8V | also GD25LQ255E; 4-byte |

## I²C EEPROMs (10 entries)

Source: `chips/i2c_chips.toml`. No JEDEC ID for I²C; chips are
identified by name only via `--chip <NAME>`. The bit-stuffing /
addr-width / page-size differences across the family are handled
automatically.

| Chip | Size | Page | Addr width | Slave-addr bits stuffed |
|---|---|---|---|---|
| 24C01 | 128 B | 8 B | 1 byte | 0 |
| 24C02 | 256 B | 8 B | 1 byte | 0 |
| 24C04 | 512 B | 16 B | 1 byte | 1 (A0 used for mem-bit 8) |
| 24C08 | 1 KB | 16 B | 1 byte | 2 (A0/A1 used for mem-bits 8-9) |
| 24C16 | 2 KB | 16 B | 1 byte | 3 (A0/A1/A2 used for mem-bits 8-10) |
| 24C32 | 4 KB | 32 B | 2 bytes | 0 |
| 24C64 | 8 KB | 32 B | 2 bytes | 0 |
| 24C128 | 16 KB | 64 B | 2 bytes | 0 |
| 24C256 | 32 KB | 64 B | 2 bytes | 0 |
| 24C512 | 64 KB | 128 B | 2 bytes | 0 |

## Adding a new chip

Most chips that aren't in the database still work via the
`--chip <NAME>` override if their pinout and opcodes match a chip
that *is* in the database (it's almost always one of the W25Q /
MX25L / GD25Q / 24Cxx clones).

For chips that need first-class support, add an entry to the right
TOML file:

```toml
[[chip]]
name = "MX25R8035F"
jedec_id = "C22814"
size_kb = 1024
page_size = 256
sector_size = 4096
erase_time_ms = 60
notes = "Macronix 8 Mbit, low-power 1.8V"
```

The `chipdb` unit tests validate that every entry has a unique
JEDEC ID (6 hex chars, all-hex) at build time, so a duplicate or
malformed ID will fail CI before the chip lands in a release.
PRs welcome at
[github.com/packetThrower/etch341](https://github.com/packetThrower/etch341).
