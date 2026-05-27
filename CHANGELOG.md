# Changelog

All notable changes to etch341. The format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/) with `0.x.y` meaning the public API
+ on-disk formats may change between minor releases.

## [Unreleased]

### Added

- **Cross-platform window titlebar** — `gpui_component::TitleBar`
  widget renders min/max/close buttons + title text on Windows and
  Linux (where `appears_transparent` hides the native chrome and
  there was previously nothing drawing the controls). On macOS the
  widget paints under the native traffic-light overlay.
  `WindowDecorations::Client` pairs with it to keep KDE Plasma's
  KWin from stacking a server-side titlebar on top.
- **Windows `.exe` icon** — `build.rs` + `resources/icon.rc` embed
  `resources/icons/icon.ico` into the PE resource section so
  Explorer / taskbar / Alt-Tab / Start menu show the etch341 icon
  on the binary itself.
- **Settings → "Open folder"** button next to the prefs path opens
  the prefs.toml directory in the OS file manager (`open`/
  `explorer`/`xdg-open`).
- **Settings → "Restore window position on startup"** toggle.
  Off by default; when on, the window's last position + size are
  saved on close and restored on next launch.
- **Stepper-style sidebar** for the canonical workflow (Detect →
  Read → Erase → Write → Verify) — diagnostic tools (Blank check,
  Hex viewer) and Settings sit below a thin divider as flat rows.
  Free-jump preserved: click any step at any time.
- **Macronix MX25L1006E / 2006E / 4006E** (1/2/4 Mbit) chip-DB
  entries. The MX25L1006E (JEDEC `C22011`) is silicon-validated
  against a Dell-OEM AMD Samoa GPU BIOS.
- **W25Q80DV silicon validation** — clean detect + double-read
  SHA match across two physical chips (Dell Precision 3520 EC
  firmware).
- **W25Q128JV silicon validation** — clean detect against a
  consolidated Intel BIOS + ME + GbE flash on a Dell laptop
  motherboard.

### Changed

- **Settings pane** no longer shows the activity log; the log was
  designed to surface op progress and steals real estate when
  you're configuring rather than operating.
- **Activity log** fills the full width of its resizable panel
  (was hugging the longest line and leaving a margin on the
  right); auto-scrolls reliably on every new line
  (`ScrollHandle::scroll_to_bottom` runs at paint time, so the
  new line is in the height by the time the bottom is computed);
  min-height raised from 80px to 120px so the splitter can't
  shrink the pane to barely-readable.
- **Op-pane buttons** tightened from `px_4 / py_2 /` default font
  to `px_3 / py_1p5 / 13px / min_w 96` — the bulky CTAs read as
  out of proportion against the rest of the chrome. Done in one
  helper (`styled_button`) so future tweaks land in a single
  place.
- **Detect button label** "Refresh" → "Detect chip" — matches
  the pane title and reads as the intended action without the
  user needing to read the body paragraph first.
- **Sidebar** narrower (220 → 180 px) and content padded to match
  the right-pane gutter; the workflow's short labels left ~80 px
  of slack at the wider size.
- **`prefs.toml` path on Windows** now resolves to
  `%APPDATA%\etch341\prefs.toml` (was hardcoded to `$HOME`, which
  Windows doesn't set — prefs never loaded or saved on Windows).
  macOS / Linux still use `$HOME/.config/etch341/prefs.toml` as
  before; no existing prefs migration needed.
- **Op-pane shell** factored into a shared `op_pane(heading,
  body)` helper; Erase + Write share `armed_warning()` and
  `armable_button()` (was duplicated). Six panes refactored,
  net −16 lines, future "what an op pane looks like" tweaks land
  in one place.

### Fixed

- **I²C page-write race** that garbled writes during silicon
  bring-up against an AT24C02N. `wait_ready` was racing the
  chip's internal write cycle; the fix sleeps through the
  datasheet `tWR` first (silicon is guaranteed busy during that
  window), then ACK-polls with a 50 ms tail for cycle stretching
  at voltage/temperature corners. Protocol matches the canonical
  embedded-hal 24Cxx drivers.
- **GNOME Wayland resize handles** were a 1-pixel diagonal
  sliver because the compositor refuses xdg-decoration and the
  gpui default client inset is 0. `window.set_client_inset(px(10))`
  widens the hit-test margin. No-op on macOS / Windows / X11.

### Documented

- **Stuck-high MISO on large SPI reads** filed in TODO after a
  W25Q128JVSQ test where read 1 of a fresh `detect` → `read`
  sequence returned ~3241 stale-`0xFF` bytes scattered across a
  4 MB range; reads 2/3/4 (including one at 750 kHz) all matched.
  Failure rate scales with USB-packet count, so smaller chips
  passed first-try. Workaround today: dump twice and `cmp`.

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
