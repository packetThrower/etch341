# Changelog

All notable changes to etch341. The format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/) with `0.x.y` meaning the public API
+ on-disk formats may change between minor releases.

## [Unreleased]

### Changed

- **I²C clock defaults to 100 kHz** (was 750 kHz, which is
  out-of-spec for every chip in the 24Cxx family). Explicit
  `-s 750` is now rejected for I²C ops with a message pointing at
  the 400 kHz datasheet ceiling. SPI ops are unaffected — the
  `-s` default for SPI is still 750 kHz. Background: an
  M24C02-R was bricked during 2026-05 bring-up because the
  global 750 kHz default exceeded its 400 kHz max mid-write and
  the chip never returned to ready.

### Documented

- **I²C clock + over-clock failure mode** in
  [Usage → I²C](https://packetthrower.github.io/etch341/usage/i2c/).
  Adds a Clock speed section explaining the default + ceiling,
  and an `Error: Timeout mid-write, chip goes silent on retry`
  entry under Troubleshooting. Plus an in-circuit write warning
  covering the bus-contention failure mode seen on a Cisco C921
  during the same bring-up.

## [0.4.0] — 2026-05-27

### Added

- **SFDP support** (`etch341 sfdp` + folded into the Detect pane).
  Reads JESD216 Serial Flash Discoverable Parameters from chips
  that carry it (most SPI NOR since ~2011) and decodes the JEDEC
  Basic Flash Parameter Table: total size, page size, address
  width, 4K erase opcode, and up to four erase types. Detect now
  always reads SFDP after JEDEC and shows both a chip-info card
  (JEDEC + chip name + source + size) and an SFDP card (raw
  decoded table) inline in the pane.
- **SFDP fallback in `resolve_chip` + GUI ops** — when JEDEC isn't
  in `chips.toml`, the read / write / erase / verify / blank-check
  / Detect paths now synthesize a `Chip` from SFDP (name like
  `"C22011 (SFDP)"`, derived size / page / sector) instead of
  hard-failing with `ChipNotRecognized`. Explicit `--chip <NAME>`
  still wins over both lookups. Silicon-validated against an
  uncatalogued MX25L1006E: SFDP-synthesized parameters produced
  the byte-identical read as the curated DB entry.
- **Settings → Read save location** picker. Read pane dumps used
  to land in `$HOME` unconditionally; now configurable via a
  folder picker in Settings with a free-form display of the
  current value. Persisted in `prefs.toml`.
- **Status registers Copy button** and **SFDP Copy button** —
  each card has a small "Copy" pill that puts the plain-text
  decoded view on the clipboard for paste-into-bug-report / share
  workflows. GPUI doesn't support cursor-based text selection in
  rendered text, so copy-all is the practical substitute.
- **Op-pane scrolling** — every pane's content now scrolls within
  the resizable pane area instead of clipping at the bottom. The
  Settings pane needed this for the WINDOW + READ SAVE LOCATION
  + PREFERENCES FILE sections to stay reachable on shorter
  windows; the SFDP card likewise can extend past one viewport.
- **Output cards** in Status registers and Detect / SFDP panes
  set "operation result" apart from the pane's heading + body +
  button stack with a subtle glass background + hairline border.
- **Screenshot of the GUI** in `docs/public/etch341.png`, embedded
  in `usage/gui.md` and the project README so visitors landing on
  either can see what the running app looks like before reaching
  for the installer.

### Changed

- **Progress indicator** in the session header now renders as an
  accent-blue pill (background tint + accent-blue text) while an
  op is running instead of the previous near-invisible
  tertiary-gray text. Easy to glance and tell whether a Read /
  Write is in progress; falls back to the quiet "idle" treatment
  when nothing's running.
- **Em-dashes** removed from every user-visible string across the
  GUI + CLI (panel bodies, armed warnings, log lines, error
  messages, SPI speed labels, Find input placeholder). Replaced
  with `:`, `.`, or `,` based on the role each em-dash was
  playing. Code comments left alone.
- **Detect pane** body rewritten in plainer language and now
  clarifies that Detect is optional (every op auto-detects on
  its own). "Other steps" matches the stepper sidebar's
  vocabulary.
- **Read pane** body trimmed of "runs in the background, watch
  the log" filler. References Settings → Read save location.

### Fixed

- **SFDP parser**: `parse_bfpt` was reading "DWORD 11" (page size
  + program/erase timings) regardless of the BFPT's actual
  length. Older pre-JESD216A chips like the MX25L1006E ship a
  9-DWORD BFPT; reading past it returned garbage that decoded
  page_size as 32768 on this chip. Now gated on
  `length_dwords >= 11` with a sanity cap to [256, 4096]. New
  `nine_dword_bfpt_defaults_page_to_256` regression test
  exercises the exact MX25L1006E pattern.
- **Activity log** typed-text-invisible bug was fixed in v0.3.1;
  this release adds the matching find-input-placeholder visibility
  fix that slipped through then.

### Documented

- **Detect / SFDP consolidation** — the standalone "SFDP" sidebar
  pane is gone; the Detect pane now does both jobs (JEDEC + DB
  lookup + SFDP read + decoded table). Updated `usage/gui.md` to
  describe the combined layout.

## [0.3.1] — 2026-05-26

### Fixed

- **Windows GUI launched a console window** alongside the GUI on
  every double-click. Rust's default `console` subsystem
  allocates a fresh console for any binary launched from Explorer;
  release `gui` builds on Windows now set
  `windows_subsystem = "windows"` so no console appears. CLI
  subcommands invoked from `cmd` / PowerShell still surface
  stdout via an `AttachConsole(ATTACH_PARENT_PROCESS)` fallback
  (output prints after the next prompt redraws, which is usable
  but ugly — a dedicated `etch341-cli.exe` console-subsystem
  sibling is the cleaner fix and remains deferred until Windows
  CLI usage justifies it).
- **Hex pane rendered "ghostly" on Windows + Linux** because
  every fixed-width region called `font_family("Menlo")`
  directly. Menlo is macOS-only; the Windows / Linux fallback
  was a thin substitute that read poorly against the dark
  theme. New `theme::MONO_FONT` constant picks `Menlo` on
  macOS, `Consolas` on Windows, `monospace` on Linux; every
  callsite in `log.rs` + `panes.rs` reads from it.
- **Find input text was invisible** (typed characters didn't
  appear) — the wrapper div carried
  `text_color(theme::bench_black())` from when gpui-component's
  Input had a white background, but after we forced
  `Theme::change(ThemeMode::Dark)` in v0.3.0 the Input went
  dark-on-dark. Drop the override; let the theme drive both
  sides.
- **`Ctrl+C` / `Ctrl+F` / `Ctrl+G` silently no-op'd on
  Windows + Linux**. GPUI treats `cmd-` and `ctrl-` as distinct
  chords; the existing bindings were `cmd-` only. Both forms
  now bind, each gated to its native platform — `cmd-*` on
  macOS only, `ctrl-*` on Windows + Linux only — so neither OS
  shadows the other's convention.

### Documentation

- **Docs header** gains a "← packetThrower" pill linking back to
  https://packetthrower.github.io/ for visitors who want to
  bounce out to the rest of the project index. Sits next to the
  existing "Docs" pill in the header.
- `chipdb.rs` module-level comment said the chip database is
  read at runtime; it's actually compiled in via `include_str!`
  at build time. Comment-only fix.

## [0.3.0] — 2026-05-26

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
- **Status register dump** — `etch341 sr` (and a matching GUI
  "Status regs" pane in the sidebar's tools section) reads
  SR1 / SR2 / SR3 and decodes the standard bit fields
  (`WIP` / `WEL` / `BP[2:0]` / `TB` / `SEC-or-BP3` / `SRP0` /
  `SRP1` / `QE` / `LB` / `CMP` / `SUS` / `ADP` / `WPS` / `DRV`
  / `HOLD-RST`). Diagnoses the two most common silent-failure
  modes: block-protect bits set (writes silently fail) and QE
  clear (quad opcodes NACK). SR1 is universal across SPI NOR
  vendors; SR2 / SR3 follow the W25Q-family convention and
  show "didn't respond" on chips that don't implement them.
  Silicon-validated on a Macronix MX25U4033E.
- **`read -o -`** dumps the chip to stdout instead of a file —
  enables `etch341 read -o - | sha256sum` and similar pipe
  idioms. Success summary lines suppressed in stdout mode so
  they don't interleave with the binary data; errors still
  surface on stderr.
- **Linux `.deb` / `.rpm` / `.pkg.tar.zst` postinstall hook** —
  reloads `udev` rules and re-triggers attached USB devices
  immediately on install so the bundled `99-ch341a.rules` takes
  effect without an unplug-replug or manual `udevadm control
  --reload`. The single
  `packaging/linux/etch341-postinstall.sh` script is wired into
  all three formats (dpkg-deb `-R`/`-b` injection for `.deb`;
  fpm's `--after-install` for `.rpm` + `.pacman`). Chroot /
  container installs no-op cleanly when `/run/udev` is absent.

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
