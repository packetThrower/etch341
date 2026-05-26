# Changelog

All notable changes to etch341. The format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/) with `0.x.y` meaning the public API
+ on-disk formats may change between minor releases.

## [Unreleased]

## [0.2.0] — 2026-05-26

### Added

- **Native installers** for every supported platform (replaces v0.1.0's
  bare-binary tarballs):
  - macOS: `.dmg` + `.app.zip`
  - Windows: NSIS `-setup.exe` + MSI (via cargo-wix, stable tags only) +
    portable bare-`.exe` zip
  - Linux: `.deb` + `.AppImage` + `.rpm` + `.pkg.tar.zst`
- **App icon** — wireframe SOIC-8 in the Baudrun + PortFinder family
  palette (silver outline, gold gull-wing leads, silver pin-1 dot on a
  navy squircle).
- **CLI inspect commands**: `chips list/find`, `strings -i <file>`,
  `search <pattern> -i <file>`.
- **`--dry-run`** now actually works (was advertised in v0.1.0 but
  silently ignored). Validates chip name + range + input file without
  opening the CH341A.
- **Chip database** expanded from 24 to 58 entries: W25X legacy line,
  W25Q\*JW 1.8V, MX25U sizes, GD25LQ 1.8V, GD25Q256, EON EN25QH, PUYA
  P25Q, ISSI IS25LP.
- **Docs site** at https://packetthrower.github.io/etch341/.

### Fixed

- macOS double-click on the v0.1.0 binary opened a Terminal window
  instead of launching the GUI. v0.2.0 ships a real `.app` bundle so
  Finder routes it through `LaunchServices`, no terminal.
- README's "No kernel drivers required" lede was wrong on Windows;
  WinUSB is a kernel driver that needs a one-time Zadig bind.
  Corrected to spell out the per-platform reality.
- CI clippy run failed on `rustc 1.95` (newer lints than the local
  toolchain). All 14 lints resolved.

### Documented

- Minimum OS versions per platform: macOS 11+, Windows 10 21H2+,
  Ubuntu 22.04+, Debian 12+.
- I²C path is implemented and unit-tested but **not yet validated
  against silicon** — explicit warning in both the feature table and
  the I²C usage section.

## [0.1.0] — 2026-05-25

Initial release.

- Cross-platform CLI/GUI flash programmer for the CH341A USB SPI/I²C
  interface, packaged as a single binary (no subcommand → GUI;
  subcommand → CLI).
- Full SPI workflow: `detect`, `read`, `erase` (full + range), `write`
  (with erase + verify), `verify`, `blank-check`. 4-byte addressing for
  chips > 16 MB.
- I²C scaffolding: protocol layer, CH341A transport, CLI subcommands,
  24Cxx chip database. Mock-tested but awaits real-EEPROM validation.
- GPUI-based desktop GUI with hex viewer, strings extraction, find /
  jump-to-offset / byte selection, live progress, persistent prefs.
- Chip database with 24 entries across W25Q, MX25L, MX25U, GD25Q,
  SST25VF, AT25SF.
- CI/CD across Linux (amd64 + arm64), macOS (arm64 + cross-compiled
  amd64), Windows (amd64 + arm64) via GitHub Actions.
- Hardware-validated against Macronix MX25U4033E (1.8V) on a GTX 1060
  VBIOS chip — full erase → write → verify cycle, byte-identical
  SHA-256 match.

[Unreleased]: https://github.com/packetThrower/etch341/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/packetThrower/etch341/releases/tag/v0.2.0
[0.1.0]: https://github.com/packetThrower/etch341/releases/tag/v0.1.0
