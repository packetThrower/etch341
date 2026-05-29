---
title: Chip database
description: Every SPI NOR flash and I²C EEPROM family etch341 recognizes by JEDEC ID or chip name. Inventory plus how to add a new entry.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/reference/chips.md
---

etch341's chip database lives in two TOML files at the repo root,
embedded into the binary at build time via `include_str!`. Running
`etch341 chips` from the CLI lists the live, in-binary set.

## SPI NOR (62 entries)

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

### Other 3.3V families

| Chip | JEDEC | Size | Manufacturer |
|---|---|---|---|
| SST25VF016B | BF2541 | 2 MB | SST (AAI word-program) |
| SST25VF032B | BF254A | 4 MB | SST |
| SST25VF064C | BF254B | 8 MB | SST |
| AT25DN512C | 1F6501 | 64 KB | Adesto (no SFDP) |
| AT25SF041 | 1F8401 | 512 KB | Adesto |
| AT25SF081 | 1F8501 | 1 MB | Adesto |
| AT25SF161 | 1F8601 | 2 MB | Adesto |
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
