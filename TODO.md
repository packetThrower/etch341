# TODO

Open work for etch341. Items are grouped by priority tier rather
than by release — a small number of high-value low-effort wins
should land before any of the big-effort items.

The "Why useful" notes are kept concrete so a future contributor
(or model) picking up a thread can verify it's still relevant
before doing the work. When something gets done, move the line into
[CHANGELOG.md](CHANGELOG.md) under the relevant release and delete
it from here.

---

## SPI flash feature gaps

Standard programmer features etch341 doesn't have yet, in
priority order.

### High value, low–medium effort

- [ ] **Force / skip-probe mode** (`--force`) — bypass JEDEC
      sanity-check for chips whose ID register is damaged or whose
      JEDEC ID is missing from the database. Today the `--chip`
      override already implies this, but `--force` would make it
      explicit + skip the JEDEC read entirely to save a roundtrip.
      ~30 min.
- [ ] **In-place fingerprint** (`etch341 fingerprint` /
      `etch341 sha256`) — hash the chip without writing a file.
      Faster than `read -o - | sha256sum` because we don't allocate
      the buffer twice. ~30 min.
- [ ] **`etch341 diff <a.bin> <b.bin>`** — show which addresses
      differ between two dump files, with an offset histogram
      ("0x00000 - 0x0FFFF: 142 differences", etc.). Useful after a
      verify-fail to understand what got corrupted (uniform random
      drift = bus noise, clustered runs = bad sector). Could be
      part of `etch341 search` as a sibling mode, or its own
      subcommand. ~1 hr.
- [ ] **Skip-equal / patch write mode** (`etch341 write --patch`)
      — read the chip first, compare to the input file, and only
      issue page-programs for sectors that actually differ. Huge
      time + chip-wear win on incremental updates (e.g.
      reflashing a BIOS where 95% of the image is unchanged from
      what's already on the chip). The compare-and-write loop is
      page-aligned so erase semantics stay clean: erase only the
      sectors that hold a differing page, not the whole chip.
      ~1-2 hr. Pairs naturally with the `etch341 diff` work above
      since both walk the same page-by-page comparison.
- [ ] **Auto-backup before destructive ops** — snapshot the chip
      to `<read-output-dir>/etch341-backup-<unix>.bin` before any
      `erase` or `write` op runs, controlled by a Settings toggle
      (and an `--auto-backup` / `--no-backup` CLI flag). Cheap
      insurance against the "I forgot to Read first" foot-gun;
      especially valuable for the GUI two-stage arm flow where
      the user might click through quickly. ~30 min.
- [ ] **`--verify-read <N>`** for `read` — do N full reads, only
      accept the dump when all N SHA-256s match. Pairs with the
      stuck-high MISO TODO in the hardware-validation section
      so the standard workflow is "the tool is the workaround"
      instead of "dump twice and `cmp` manually". Default `N=1`
      preserves current behaviour. ~30 min.
- [ ] **`etch341 uid`** — read the 64-bit factory-unique ID via
      opcode `0x4B`. Per-chip serial number useful for inventory
      / fingerprinting / per-chip key derivation. Surface as an
      8-byte hex string. ~15 min. GUI surfaces it in the Detect
      pane's chip-info card next to the JEDEC + chip name fields.
- [ ] **`etch341 reset`** — send the standard SPI reset sequence
      (`0x66` Enable Reset + `0x99` Reset). Recovers chips stuck
      in 4-byte address mode, suspended-erase state, or
      mid-program limbo after a botched op without the user
      having to power-cycle the CH341A. ~15 min.

### Medium value, medium effort

- [ ] **Write-protection management** — `etch341 wp status`,
      `etch341 wp enable`, `etch341 wp disable`,
      `etch341 wp range <start:len>`. Drives the BP0-3 + SRP bits in
      SR1/SR2 across the common chip families (Winbond, Macronix,
      GigaDevice all use roughly the same conventions, with
      manufacturer-specific quirks). Critical for shipping
      locked-down firmware and for unlocking factory-protected
      chips. ~2 hr to cover the W25Q / MX25L / GD25Q families;
      more if we want to span every chip in the DB exhaustively.
- [ ] **Probe via alt opcodes** — try `0xAB` (Release Power Down /
      Read Device ID) and `0x90` (Read Manufacturer / Device ID,
      with address bytes) when `0x9F` returns garbage. Some older
      chips don't respond to `0x9F`. ~1-2 hr.
- [ ] **OTP / security register access** — most modern SPI NOR
      chips carry a separate 256-3072 byte one-time-programmable
      region behind dedicated opcodes (`0x48` read security
      register, `0x42` program, `0x44` erase on W25Q-family).
      Common uses: per-board serial numbers, MAC addresses,
      vendor keys. Start with read-only access (`etch341 otp
      read`) since the program/erase paths are genuinely
      one-time-only and warrant their own arm/confirm flow.
      ~2-3 hr.
### Big effort, big payoff (when there's a real need)

- [ ] **Region / layout support** — `etch341 read --region BIOS`,
      `etch341 write --include BIOS,ME`, etc. A layout file
      describes named regions (`BIOS`, `ME`, `GBE`, `PD` on Intel
      chipsets) as `start:end:name` lines; ops can target one or
      several by name. Critical for modern Intel motherboards
      where touching the ME region risks bricking. ~5 hr
      including a layout-file parser, a region-overlay engine,
      and updated verify-after-partial-write semantics.
- [ ] **IFD (Intel Flash Descriptor) parsing** — recognise the
      magic header at offset 0x10 (`0x0FF0A55A`), parse the
      descriptor, auto-derive region boundaries instead of
      requiring a hand-written layout file. Built on top of the
      region/layout work. Reports ME version, FD version, and
      lock state. ~3-4 hr.
- [ ] **Multi-chip on bus** (`--chip-select 1 / 2`) — pick between
      two SPI chips wired to the same CH341A on D0 vs an external
      GPIO. Useful for dual-BIOS motherboards. The standard
      CH341A breakout only exposes one CS, so this needs the user
      to wire an external switch — limited audience. ~2 hr.

### Low value / out of scope

- **Other programmer hardware** (FT2232, Bus Pirate, Raspberry Pi
  GPIO, dedicated commercial programmers, ...) — would need a
  major refactor of the `SpiTransport` trait into a programmer-
  pluggable system. The CH341A focus is etch341's whole identity.
- **93xx microwire EEPROMs** — different protocol from SPI NOR
  and from 24Cxx I²C. Small audience; `eeprom-prog` or
  `minipro` cover this.
- **JTAG / SVF flashing** — different domain.

---

## Hardware validation gaps

The mock test suite catches protocol-layer regressions but not
hardware-protocol mismatches. These paths are implemented and
unit-tested but haven't seen silicon yet:

- [ ] **I²C write path — clean-chip silicon validation** — first
      contact (AT24C02N on an old DVI graphics card) confirmed the
      probe/scan ACK-polarity assumption and round-tripped 256-byte
      reads with stable SHA-256 across two passes. The write path
      uncovered a real race: `wait_ready` was timing out before the
      chip's tWR cycle finished, garbling subsequent page writes.
      The fix (sleep tWR first, then ACK-poll with a 50 ms window)
      brings our protocol in line with what the standard embedded-
      hal 24Cxx drivers do. Bring-up iteration on a clip-attached
      20-year-old part corrupted the chip beyond clean retest, so
      a final write-then-read-back integrity loop on a fresh chip
      is still owed. The on-wire transaction shape is already
      validated against the canonical 24Cxx page-write sequence
      (slave|W → addr_bytes → data → STOP) — what's left is the
      "do it once cleanly against a healthy chip" loop.
- [ ] **SPI 4-byte addressing (>16 MB chips)** — the W25Q256JV
      and MX25L25635F + family entries use the 4-byte opcode
      variants (`0x13` / `0x12` / `0x21` / `0xDC`) but no chip in
      this size range has been physically tested.
- [ ] **Silent stuck-high MISO on large reads** — silicon test on
      a W25Q128JVSQ (16 MB, JEDEC `EF4018`) showed read 1 of a
      fresh `detect` → `read` sequence return ~3241 stale-`0xFF`
      bytes scattered across a ~4 MB range; reads 2/3/4 (including
      one at 750 kHz) all matched each other (SHA `68ba78ad…`).
      For a 16 MB chip we send ~540,000 USB IN packets of 31 bytes
      each; a single packet returning the requested length with
      `0xFF` padding (vs the real MISO data) is indistinguishable
      from real erased flash and silently corrupts that chunk. The
      smaller chips tested so far (1 MB W25Q80DV ×2, 128 KB
      MX25L1006E) all matched on first try — so failure rate scales
      with packet count. Fix candidates: (a) add an internal double-
      read-and-compare mode behind a `--verify-read` flag for
      critical dumps; (b) use `FAST_READ` (`0x0B`) with its dummy
      byte to give the chip an extra cycle of timing margin; (c)
      shorter chunks to limit the blast radius of any one bad
      packet. The 3-out-of-4 reads matching gives a clean workaround
      today: always dump twice and `cmp` before trusting the result.

---

## GUI / UX

The GUI works but feels heavier than it needs to. Items in this
section are about visual + interaction polish rather than new
functionality.

- [ ] **Diff view after failed verify** — when verify fails,
      switch the Hex pane to highlight differing addresses and
      let the user step between them with Cmd+G. Closes the loop
      between Verify and Hex. ~30 min.

## CLI / general polish

- [ ] **`-V` / `-VV` verbosity levels** — today `-v` is binary
      on/off. Multi-level verbose (header info → bus bytes → full
      USB-packet hex) would let users dial detail to what they
      need; the bare `-v i2c scan` dump can be overwhelming.
- [ ] **Split `etch341-cli.exe` from `etch341.exe` on Windows** —
      the GUI binary is marked `windows_subsystem = "windows"` so
      Explorer launches don't pop a console, with an
      `AttachConsole(ATTACH_PARENT_PROCESS)` fallback that grafts
      onto the parent shell when invoked as a CLI. That fallback
      works but the shell has already fire-and-forgotten the
      process by the time the output prints, so `etch341 detect`
      from PowerShell lands its lines *after* the next prompt has
      redrawn — usable but ugly. PortFinder solved the same
      problem by shipping a second binary
      (`portfinder-cli.exe`, console subsystem). Mirror the
      pattern: split `src/main.rs` into a lib + two
      `src/bin/etch341*.rs` entry points, declare two `[[bin]]`
      targets in `Cargo.toml`, and bundle both `.exe`s into the
      NSIS / MSI / portable-zip artifacts. Update the Scoop
      manifest's `bin` array to surface both shims on PATH.
      ~1-2 hr. macOS / Linux unaffected (no subsystem distinction).
- [ ] **Fast hash alternative** (`--hash xxh3` or `--hash crc32`)
      for `read` / `fingerprint` — SHA-256 over a 16 MB dump
      runs ~80 ms; xxh3 over the same data runs ~3 ms. Default
      stays SHA-256 (cryptographic confidence on shared / pasted
      hashes), but a fast option helps with "did anything
      change?" round-trips during dev. The output line just
      switches algorithm label. ~30 min.
- [ ] **CLI `--from-chip`** for `strings` / `search` — let them
      operate directly on the live chip instead of needing a `read`
      first. Just plumbing: open Ch341, run ops::read into a Vec,
      then dispatch to the existing inspect functions. ~45 min.

---

## Distribution / release plumbing

- [ ] **Apple Developer cert + notarization** — currently the
      macOS `.dmg` ships ad-hoc signed (`signing-identity = "-"`).
      First-launch is the right-click → Open dance. With a real
      cert + notarization, double-click works as expected from a
      fresh download. Costs $99/yr for the Developer ID + the
      workflow plumbing to run `notarytool submit` after the
      `.dmg` is built. Add as GitHub secrets.
- [ ] **Windows Authenticode cert** — the NSIS `-setup.exe` and
      MSI both trigger SmartScreen warnings on first run on a
      fresh machine. A real cert removes that. ~$300/yr+; same
      add-as-secrets workflow change.
- [ ] **winget manifest submission** — once the MSI is shipping
      reliably, submit to `microsoft/winget-pkgs` so
      `winget install packetThrower.etch341` works. Needs the
      ARP properties already set in `main.wxs`.

---

## Tests / CI / docs

- [ ] **Hardware-tagged integration tests** — the
      `#[cfg(feature = "hardware")]` gate exists but no tests
      have been written for it. Would let us run end-to-end
      detect → read → erase → write → verify against a real
      MX25U4033E in CI on a self-hosted runner with a CH341A
      attached. Out of scope until self-hosted runner infrastructure
      exists.
- [ ] **Google Search Console verification token** — add the
      `google-site-verification` meta tag to `docs/astro.config.mjs`
      after claiming the property at
      <https://search.google.com/search-console>. The slot is
      reserved with a comment in the config file.
- [ ] **Docs OG image rebuild via build-og-image.mjs** — the
      first cut was generated inline via rsvg-convert with a
      hand-stamped SOIC silhouette inside the OG SVG. The
      `docs/scripts/build-og-image.mjs` script (which uses the
      canonical icon) hasn't been run yet because we didn't have
      `pnpm install` complete at the time. Run it once after a
      fresh `pnpm install` and commit the resulting PNG.
