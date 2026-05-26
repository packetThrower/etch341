---
title: Wiring + voltage
description: How to wire the CH341A to an SPI flash or I²C EEPROM. Clip orientation, the 3.3V/5V jumper, the 1.8V switch on V1.7 modules, and recovery paths for in-circuit programming.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/usage/wiring.md
---

The CH341A black module connects to a chip in one of three ways:

1. **ZIF socket** — insert a loose 8-pin chip directly. Most
   reliable; no clip-orientation guessing.
2. **SOIC-8 clip via ribbon cable + breakout** — clip onto a chip
   that's still soldered to a board, ribbon cable carries the
   signals to a small adapter PCB that plugs into the ZIF socket.
3. **Direct flying-wire breadboard** — wire each CH341A pin
   header to the chip with individual jumpers. Slow; useful when
   nothing else fits.

## SOIC-8 pinout

Looking down at the chip with the dot/notch (pin 1 marker) at the
top-left:

| Pin | SPI (25xxx) | I²C (24Cxx) |
|---|---|---|
| 1 | CS# | A0 |
| 2 | DO / MISO | A1 |
| 3 | WP# (write protect) | A2 |
| 4 | GND | GND |
| 5 | DI / MOSI | SDA |
| 6 | CLK | SCL |
| 7 | HOLD# / RESET# | WP# (write protect) |
| 8 | VCC | VCC |

The **red wire of the SOIC clip** marks pin 1. About half of
first-try failures are clip-reversed — always double-check
orientation against the package marker.

## CH341A pinout

The same CH341A board serves both SPI and I²C, but the signals
land on different pins per mode:

| Pin | SPI | I²C |
|---|---|---|
| D0 | SCS (chip select) | **SCL** |
| D1 | — | **SDA** |
| D3 | SCK | — |
| D5 | MOSI | — |
| D7 | MISO | — |

The ZIF socket on the black module is internally wired to route
the right CH341A pins to the right socket positions for each chip
family — that's why a 24XX chip placed in the 24XX position works
even though SPI and I²C use different CH341 pins.

When using a SOIC clip + breakout, the breakout is the bit that
maps clip wires to ZIF positions. Make sure it's plugged into the
ZIF half that matches your chip family:

- **SPI flash (25xxx)** → breakout in the **25XX position**
- **I²C EEPROM (24Cxx)** → breakout in the **24XX position**

If you're moving between an SPI chip and an I²C chip without
moving the breakout, etch341 won't be able to talk to the new
chip — see the [I²C troubleshooting
section](/etch341/usage/i2c/#troubleshooting).

## Voltage

The CH341A black module has a **3.3V/5V jumper** near the USB end.

- **3.3V** — correct for every 3.3V chip in the
  [database](/etch341/reference/chips/) (W25Q, W25X, MX25L, GD25Q,
  SST25VF, AT25SF, EN25QH, P25Q, IS25LP).
- **5V** — **will damage every chip in the database**. Do not flip
  the jumper to 5V unless you know exactly why.

For the **1.8V chips** in the database (W25Q\*JW, MX25U, GD25LQ),
the standard 3.3V/5V jumper isn't enough — these need a real 1.8V
rail. Two options:

1. **CH341A V1.7 module** — the V1.7 has a separate switch
   alongside the 3.3V/5V jumper that selects 1.8V. Flip the
   3.3V/5V jumper to 3.3V *and* flip the V1.7's 1.8V switch on,
   and the output rail becomes 1.8V.
2. **Level-shifter adapter** — a separate breakout PCB that sits
   between the CH341A and the 1.8V chip, regulating the supply
   rail to 1.8V and translating logic levels accordingly. Several
   vendors sell these; the typical V1.7 module obsoletes the need
   for most users.

Plugging a 1.8V-only chip into a 3.3V programmer over-volts every
input pin and can permanently damage the chip in seconds. Always
check the chip's datasheet for its actual supply range before
hooking anything up.

## Sanity check: VCC reaches the chip

Before running any operation, especially in-circuit, take 10
seconds and probe pin 8 of the chip with a multimeter (red probe
to pin 8, black probe to any GND on the board, with the CH341A
plugged in to USB):

- **~3.3V** — power is good (or ~1.8V for U-series chips in 1.8V
  mode).
- **0V** — clip's not seated, the wire to pin 8 broke, or
  in-circuit there's a power switch in series with the chip's
  supply.
- **Anything between 0V and 3.3V** — the in-circuit host is
  partially powering the rail through some other path. Fighting
  the programmer; lift pin 8 from the PCB to isolate.

## In-circuit programming

Clipping a chip that's still soldered to a board is convenient but
runs into the host's other power and signal paths.

### MISO stuck low (`0x000000`)

The host's SPI controller actively drives MISO low even when the
board is "powered off". `etch341 detect` returns
`JEDEC ID : 0x000000` and verbose mode shows clean command bytes
going out but nothing meaningful coming back.

Diagnosis:

```sh
etch341 detect -v        # with the clip OFF the chip — nothing else changed
```

- `<- IN [4]: ffffffff` → the CH341A is fine; the target board is
  the culprit. Move to the recovery steps below.
- `<- IN [4]: 00000000` → CH341A or wiring issue, not the target.

Recovery, in order of effort:

1. **Disconnect the host's power completely.** Pull the AC plug
   and any battery; let any standby caps bleed for 30 seconds.
   Some boards still drive MISO from a 3.3V_AUX rail.
2. **Lift pin 7 (HOLD# / RESET#)** of the chip from the PCB pad
   with a hobby knife — the chip's internal pull-up then keeps
   HOLD# high. Reversible.
3. **Lift pin 8 (VCC)** and inject 3.3V externally so the chip is
   fully isolated from the board's supply path. Reversible.
4. **Desolder the chip** entirely and program it in the ZIF
   socket. Most reliable; needs a hot-air station.

### HOLD# being held low

Some game cams / IoT devices park their flash by driving the
chip's HOLD# pin (pin 7) low — the chip ignores all SPI commands
until HOLD# goes high. Symptom is the same as MISO stuck low; the
recovery is the same (lift pin 7).

## Is the chip really an I²C EEPROM?

Not every 8-pin SOIC on a board is an EEPROM. PCIe risers, mining
gear, and many cheap consumer boards have SOIC-8 **buck regulator
controllers** that look identical from the outside. If `i2c scan`
returns empty and the chip has a marking like `MT…`, `MP…`, or
`AOZ…`, it's probably a switching regulator. Tell-tale signs
nearby:

- A large cylindrical inductor (matching the chip's pin spacing
  for the SW node)
- Tantalum or large-value ceramic input/output capacitors
- A small Schottky diode

EEPROMs typically sit alone, with only small bypass caps. If the
chip is between two big caps and an inductor, it's almost
certainly a regulator, not memory.
