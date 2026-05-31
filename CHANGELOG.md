# Changelog

All notable changes to etch341. The format follows [Keep a
Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/) with `0.x.y` meaning the public API
+ on-disk formats may change between minor releases.

## [Unreleased]

### Added

- **Side-by-side diff view for a failed verify (GUI).** When a SPI
  verify finds mismatches, a "View diff in Hex" button opens a
  side-by-side comparison in the Hex pane: the file you verified
  against on the left (red), the chip's read-back on the right
  (green), showing only the differing regions plus a couple of
  context lines rather than the whole image. Differing bytes are
  tinted on both sides; Prev/Next chevrons (and Cmd/Ctrl+G) jump
  between regions, and bytes on either side are selectable and
  copyable like the main Hex viewer. The red/green are fixed standard
  diff colours, independent of the chosen accent.
- **`diff` and `verify --diff` on the CLI.** The same byte-level
  comparison now has a command-line surface. `etch341 diff A B`
  compares two files offline (no hardware), printing only the differing
  regions as a side-by-side hex dump — red for the left file, green for
  the right — and exits 1 when they differ / 0 when identical, so it
  slots into scripts like `diff(1)`/`cmp(1)`. `etch341 verify --diff`
  adds that same region view to a hardware verify (file vs chip
  read-back) instead of only a count and first differing address.
  Colour follows the terminal: on for a TTY, off when piped or with
  `NO_COLOR` set. The GUI view, `diff`, and `verify --diff` share one
  region-grouping core, so all three highlight identically.
- **46 more SPI NOR chips** — the database grows from 70 to 116
  entries, adding whole vendor families that were missing: Spansion /
  Cypress / Infineon **S25FL** (a networking / industrial / automotive
  staple), Micron / Numonyx **N25Q + MT25Q** (FPGA config flash),
  Microchip **SST26VF**, ISSI **IS25WP** (1.8V) plus more **IS25LP**,
  **XTX XT25F** (ubiquitous in ESP / WiFi modules), **Zbit ZB25VQ**,
  **Boya BY25Q**, Macronix **MX25R** (wide-Vcc low-power) and the
  512 Mbit / 1 Gbit **MX25L51245G / MX66L1G45G**, and EON **EN25Q**.
  JEDEC IDs were cross-referenced against published vendor ID tables
  and are flagged not-yet-silicon-tested in the notes. The
  JEDEC→voltage map learned the IS25WP (9D70) and MT25QU (20BB) 1.8V
  families so the GUI Voltage column stays correct.

### Fixed

- **Pop-out windows actually re-dock on Wayland now.** The 0.5.0 fix
  hooked `on_window_should_close`, but with client-side decorations a
  pop-out's own title-bar close button calls `remove_window()` directly
  and never routes through that callback — so on Wayland the inline log
  still didn't come back. The re-dock is now also wired to the title
  bar's `on_close_window` handler (the path the Linux close button
  actually takes); `on_window_should_close` stays for the macOS /
  Windows / compositor-initiated close paths, so both decoration modes
  are covered. Applies to the activity-log pop-out and the
  chip-database browser.

## [0.5.0] — 2026-05-30

First stable release of the 0.5.0 line — now picked up by the Homebrew
tap, Scoop bucket, and the in-app update check (the betas weren't).
Folds in everything from 0.5.0-beta.1 and 0.5.0-beta.2 below, plus:

### Added

- **I²C EEPROMs in the GUI** — the full 24Cxx workflow now has a GUI,
  closing the last CLI↔GUI parity gap. A bus toggle at the top of the
  sidebar swaps the whole tool set between SPI and I²C; the I²C side
  gets a chip dropdown (I²C has no JEDEC autodetect, so the chip is
  picked explicitly) plus **Scan / Read / Write / Verify / Erase /
  Blank check** panes that mirror the SPI workflow — same stepper
  rail, same two-stage arm/confirm on the destructive ops (Write /
  Erase), same shared Hex viewer + Settings. I²C clock is held at
  100 kHz. Scan lists the ACKing 7-bit addresses; the chip choice
  persists as you move between panes.
- **In-pane result lines for every operation (both buses)** — each op
  now reports its outcome as a coloured line in the pane itself, not
  only in the activity log: a green ✓ on success (e.g. "Read 256
  bytes from 24C02 → …", "Wrote … (verified)", "Chip matches the
  file", "… is blank — all 0xFF") and a red ✗ on failure — hardware
  errors, verify mismatches, and blocked clicks ("Pick a chip first",
  "Pick an input file first"). Detect / Status / Security-register
  reads keep their result card on success and surface the red ✗ line
  on error. The line hugs its text, wraps a long path, and clears on
  navigation so it stays scoped to the pane that produced it.

### Changed

- **`Programmer` dispatch layer over the CH341 backend** (internal).
  The high-level SPI / I²C ops were already generic over the
  `SpiTransport` / `I2cTransport` traits; the concrete `Ch341::open*`
  call sites now go through a `Programmer` enum that owns the device
  and forwards the transport calls. No behaviour change — it's the
  seam for adding a second USB bridge (a new enum variant backed by
  its own module implementing the two traits) without touching `ops`
  / `spi` / `i2c`.

### Fixed

- **Pop-out activity log now re-docks on Wayland.** Closing the
  detached log window didn't restore the inline log on Wayland
  (Ubuntu's default session). The re-dock was tied to the log
  window's entity teardown — which gpui runs synchronously on
  macOS / Windows but *defers to a later event-loop turn on Wayland*,
  so on an idle app it never fired and the inline log stayed gone.
  (The earlier `0.5.0-beta.2` fix tested on X11, where the window is
  torn down promptly, so it looked complete.) The re-dock now hooks
  the window's close *request* (`on_window_should_close`), which fires
  synchronously on every backend — X11 on `WM_DELETE_WINDOW`, Wayland
  on `xdg_toplevel::Close` — so it no longer depends on entity-drop
  timing. The same fix is applied to the chip-database browser window,
  which shared the bug (its stale handle would otherwise block
  reopening the browser on Wayland). (#1)

## [0.5.0-beta.2] — 2026-05-29

### Added

- **Chip-database browser in the GUI** — a new window (Detect →
  "Browse chip database") that lists every bundled SPI + I²C chip
  with a vendor dropdown, a live name/JEDEC search, and a
  colour-coded **Voltage** column. SPI parts show their single rail
  (green 3.3V, amber 2.3V, red 1.8V), derived from the JEDEC
  manufacturer/family byte so it can't drift from the id that
  determines it; I²C 24Cxx show the family's wide 1.8–5.5V range —
  they're the 5V-capable parts, commonly run at 3.3V or 5V on the
  CH341A. The CLI `chips` table gains a matching `VOLT` column on
  both buses, keeping the two surfaces in parity.
- **AT25DN512C** in the chip database (Adesto/Atmel 512 Kbit,
  2.3V, dual-read, JEDEC `1F6501`). This part exposes no SFDP
  table, so it needs an explicit database entry to be recognized
  by name — silicon-confirmed against a real chip.
- **Eight more Adesto / Atmel AT25 parts** rounding out the
  family: AT25SF321 (4 Mbit); the AT25DF series AT25DF041A /
  081A / 161 / 321A / 641 (4–64 Mbit, 3.3V); and the 1.8V
  AT25SL321 / AT25SL128A (32 / 128 Mbit). JEDEC IDs were
  cross-referenced across independent published vendor ID tables
  (datasheet-sourced, not yet silicon-tested — `detect` flags them
  as such in the notes). The 1.8V SL parts need a 1.8V-capable
  programmer. SPI NOR database is now 70 entries.

### Changed

- **GUI read dumps get human-readable filenames** —
  `etch341-read-<date>_<time>.bin` in local time (e.g.
  `etch341-read-2026-05-29_14-03-07.bin`) instead of the raw
  seconds-since-epoch suffix. Sorts chronologically, uses hyphens
  (not colons) so it's legal on Windows and tidy in Finder.

### Fixed

- **I²C writes no longer always time out.** Every I²C `write` /
  `erase` aborted with `Error: Timeout` because the post-page
  "wait for write cycle to finish" step polled the chip through a
  probe that infers presence from a data byte — and on a blank chip
  (or any page whose next byte is `0xFF`) that read `0xFF` and
  concluded "never ready". The CH341 never exposes the I²C ACK bit,
  so there's nothing to poll; writes now wait out the worst-case
  datasheet write-cycle time instead.
- **I²C reads no longer corrupt past ~30 bytes.** A multi-byte read
  was clocked with a single `IN | n` substream, which makes the CH341
  ACK *every* byte including the last — so the read was never
  terminated with a NACK and the chip stayed mid-transfer, shifting
  the bytes of the next chunk. Reads now clock one `IN | 1` per byte
  (master ACKs each) and a final bare `IN` (master NACKs the last),
  matching how working CH341 I²C drivers do it; the per-chunk size
  dropped from 31 to 20 so the longer request stream still fits one
  32-byte CH341 packet. With both this and the write-cycle fix, the
  full I²C `read` / `write` / `erase` / `verify` / `blank-check`
  cycle is byte-exact on real silicon (AT24C02, 100 kHz and 20 kHz)
  — the first end-to-end silicon validation of the I²C path.
- **`i2c scan` now explains the blank-chip blind spot.** A blank
  EEPROM (all `0xFF`) is indistinguishable from an empty bus on the
  CH341 (no ACK-bit readback), so `scan` can't list it; the
  empty-result message now says so and points at `--chip`.
- **Pop-out activity log now re-docks on Linux/X11** when its
  window is closed. The re-dock was wired through
  `on_window_should_close`, which gpui doesn't route from the X11
  window manager's close button; it's now tied to the log window's
  entity teardown, which fires however the window closed. (#1)
- **Activity log stays pinned to the newest line when the log pane
  is resized.** Dragging the splitter changed the viewport height
  without re-requesting the paint-time scroll-to-bottom, so the
  view drifted off the latest entry. (#2)

## [0.5.0-beta.1] — 2026-05-28

First beta of the 0.5.0 line — a big GUI pass plus OTP support.
Pre-release: not picked up by the Homebrew tap / Scoop bucket (they
track stable) or the in-app update check. Grab it from the Releases
page if you want to try it.

### Added

- **OTP / security register access** (`etch341 otp read` /
  `otp erase` / `otp write` + a Security-regs pane in the GUI).
  Reads, erases, and programs the three 256-byte security
  registers carried by the Winbond W25Q / GigaDevice GD25Q
  families (opcodes `0x48` / `0x44` / `0x42`). These commonly hold
  serial numbers, MAC addresses, or vendor keys.
  - **Read** dumps all three as offset / hex / ASCII; blank
    (all-`0xFF`) registers collapse to a one-line note. The GUI
    card's Copy button yields the same text as the CLI.
  - **Erase / write** target one register. Both are read-back
    verified, and `write` does *not* erase first (programs only
    clear bits — erase the register first for a clean write). The
    CLI gates them behind `--yes`; the GUI uses the same two-stage
    arm/confirm as the Erase / Write panes.
  - Erase + reprogram stay repeatable: etch341 never sets the
    one-time lock bits (LB in SR2), so locking a register closed
    is a deliberate non-goal.
  - GUI parity note: the GUI writes from offset 0 of the selected
    register; use the CLI's `otp write --start` for a partial
    write at an offset.
  - W25Q / GD25Q `0x48` convention only — Macronix's single
    security register (opcode `0x2B`) isn't covered.
- **Selectable accent color** (Settings → Appearance). Pick from
  eight curated swatches; the whole UI recolors live — buttons,
  selections, toggles, radio dots, sidebar — and the choice
  persists. Button labels switch between dark and light per accent
  so they stay legible.
- **New-version check** — on launch etch341 checks the project's
  GitHub releases and, if a newer stable version exists, paints a
  dot on the ⚙ Settings sidebar item. Settings → Updates has the
  status, a "View release" link, a "Check now" button, and an
  on/off toggle. Detection only — it never downloads or installs.
- **Pop-out activity log** — a ⧉ chip detaches the log into its own
  window that stays live (and follows new lines); a × chip clears
  it.
- **Activity-log timezone toggle** (Settings → Log timestamps) —
  render timestamps in local time or UTC; storage stays UTC.
- **Hex viewer font sizing** — Cmd/Ctrl + `=` / `-` / `0` zoom the
  hex and strings views independently, with size controls in
  Settings.
- The installed **version** now shows next to the Settings heading.

### Changed

- **GUI restyle** — Settings, the OTP pane, and the Write / Verify
  panes now group their controls in outlined cards with bordered,
  input-style file fields and capped content width. CTA buttons are
  a notch smaller. Consistent visual language across the app.

## [0.4.1] — 2026-05-27

Emergency point release. The 750 kHz `-s` default in 0.4.0 (and
every prior version) is ~2× the spec'd max for every chip in the
24Cxx family and was confirmed to brick an M24C02-R mid-write
during bring-up. Upgrade if you use any `i2c *` subcommand.

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

[Unreleased]: https://github.com/packetThrower/etch341/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/packetThrower/etch341/compare/v0.5.0-beta.2...v0.5.0
[0.2.0]: https://github.com/packetThrower/etch341/releases/tag/v0.2.0
[0.1.0]: https://github.com/packetThrower/etch341/releases/tag/v0.1.0
