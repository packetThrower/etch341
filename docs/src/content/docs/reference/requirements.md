---
title: Requirements
description: Minimum OS versions, supported architectures, hardware needed, and the toolchain for building etch341 from source.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/reference/requirements.md
---

## Operating system

| Platform | Minimum | Architectures |
|---|---|---|
| **macOS** | 11 (Big Sur) | arm64 (Apple Silicon), amd64 (Intel) |
| **Windows** | 10 21H2 (x64) / 11 (arm64) | amd64 (x64), arm64 |
| **Linux** | Ubuntu 22.04, Debian 12, Fedora 40; Arch &amp; openSUSE Tumbleweed (rolling) | amd64, arm64 |

The minima track GPUI's supported configurations and the CRT runtime
the prebuilt binaries link against. Older systems may work if you
build from source, but aren't tested.

The GUI uses GPUI's Vulkan backend on Linux; you need a
Vulkan-capable GPU with current Mesa drivers (most desktop and
laptop integrated graphics from the last 5+ years qualify). The
headless CLI (`etch341 --no-default-features` from source, or any
`etch341 <subcommand>` against the released binary) has no graphics
requirements and runs on any of the OSes above.

## Hardware

| | |
|---|---|
| **Programmer** | CH341A USB programmer (VID `0x1A86`, PID `0x5512`). The common "black module" and the V1.3 / V1.7 mini variants with on-board ZIF socket all work. |
| **1.8V chips** | Need a 1.8V-capable programmer — the CH341A V1.7's separate 1.8V switch, or a level-shifter adapter. The standard 3.3V/5V jumper on the black module isn't enough. |
| **Connection** | SOIC-8 clip for in-circuit programming, or insert the chip directly into the ZIF socket. Both work. |

The chip itself can be SPI NOR flash (any of the
[62 entries in the chip database](/etch341/reference/chips/), plus
unknown chips via `--chip` override) or a 24Cxx I²C EEPROM (10
families in the database).

## Driver setup

- **macOS** — no driver setup. libusb is statically linked into the
  binary and macOS leaves the CH341A's vendor interface alone.
- **Linux** — install the udev rule from `platform/udev/99-ch341a.rules`
  (or let the `.deb` / `.rpm` / `.pkg.tar.zst` install do it for
  you). Without the rule, unprivileged users hit `PermissionDenied`
  opening the device.
- **Windows** — one-time WinUSB binding via Zadig. See the
  [Install → Windows](/etch341/install/#windows) section.

## Build from source

```sh
git clone https://github.com/packetThrower/etch341.git
cd etch341
cargo install --path .                           # CLI + GUI
cargo install --path . --no-default-features     # CLI only
```

| | |
|---|---|
| **Rust** | 1.85 or newer (the project uses the 2024 edition) |
| **C compiler** | cc / clang — `rusb` compiles a vendored libusb from source |
| **Linux extras** | `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libwayland-dev`, `libx11-dev`, `libxcb1-dev`, `libxcb-randr0-dev`, `libxcb-xkb-dev`, `libxcb-cursor-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libxcb-render0-dev`, `libfontconfig1-dev`, `libfreetype-dev`, `pkg-config` — these are GPUI's compile-time pkg-config deps |

The `--no-default-features` CLI build skips the entire GPUI graph,
so it builds much faster and produces a much smaller binary (~5 MB
vs. ~50 MB) for headless / scripted use.
