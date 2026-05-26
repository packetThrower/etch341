---
title: GUI tour
description: A walkthrough of etch341's desktop GUI — what's in each pane, how the hex viewer's Find bar works, and the chip-detect / read / write / verify flow.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/usage/gui.md
---

Launching the binary with no subcommand opens the desktop GUI. On
the released installers this is what double-clicking the app does
(or running it from the Start menu / `gtk-launch etch341` on
Linux). From a source build, `cargo run` does the same.

## Layout

The window is split into three regions:

- **Sidebar** (left) — vertical pane selector. Click an item to
  switch the main view between Detect / Read / Erase / Write /
  Verify / Hex / Blank / Settings.
- **Main pane** (top right) — the current pane's controls and
  content.
- **Activity log** (bottom) — chronological log of operations,
  results, and errors. Resizable: drag the splitter to give it more
  or less vertical space. The height is persisted across launches.

The window's own size + position is also persisted, so it opens
where you left it.

## Header

The top header always shows the connection state and the active
operation's progress bar (when one is running). Click the chip name
to re-run JEDEC detection — useful after swapping chips without
restarting.

## Detect

The simplest pane. Clicks the "Refresh" button → reads the JEDEC
ID via opcode `0x9F` → looks up the chip in the embedded database.
The log shows the raw JEDEC bytes; the header shows the friendly
chip name + capacity once a match lands.

`MISO floats high` (returns `0xFFFFFF`) means the chip isn't
responding — typically clip orientation, missing VCC, or a held
HOLD#/CE# pin. `MISO stuck low` (`0x000000`) means an in-circuit
host controller is actively pulling the line low. See
[Wiring + voltage](/etch341/usage/wiring/) for the recovery paths.

## Read

Picks a known chip, reads its full contents to a file. The file
goes to a timestamped path under `~` by default; the Hex pane can
then open that file for inspection.

## Erase, Write, Verify

Each of these is **arm-then-confirm**: first click sets the button
to "Click again to confirm" with an amber warning banner; second
click within the same pane visit fires the actual destructive
operation. Switching panes resets the arm state — there's no way to
accidentally erase a chip by mis-clicking once.

Write does erase + program + verify in one pass by default (the
CLI `--no-erase` / `--no-verify` flags aren't exposed in the GUI
because the destructive variants are the rare cases).

## Hex

The Hex pane has its own internal toggle between **Hex view**
(per-byte hex + ASCII columns, virtualised so it scrolls 32 MB
files smoothly) and **Strings view** (extracted printable ASCII
runs, with offset on the left).

### Find bar

The unified Find input sits below the Browse button and applies to
both Hex and Strings views.

- `55 AA` or `55AA` → searches for the hex byte pair `0x55 0xAA`.
- `Award BIOS` → searches for the ASCII string (case-insensitive
  on letters, exact for non-letters).
- `0x10000` → jumps to that offset.

When matches are found, the chevron buttons step between them and a
counter shows `i+1 / N`. The matched bytes get a blue tint in the
Hex view; matching strings get bolded in the Strings view.

### Byte selection + Cmd+C

Click a byte to anchor a selection. Drag (within a row, or across
rows) to extend it. Shift+click to extend the existing selection
without resetting the anchor. The footer shows
`Selection: 0x{lo}..0x{hi} ({N} bytes)`. **Cmd+C** copies the
selected range as space-separated upper-case hex to the system
clipboard. Cmd+C only fires when the Hex pane is the visible pane
— typing Cmd+C in any Input still gets normal text copy.

### Keyboard shortcuts

| | |
|---|---|
| **Cmd+F** (Ctrl+F on Linux/Windows) | Focus the Find input |
| **Cmd+G** / **Cmd+Shift+G** | Next / previous match |
| **Cmd+C** | Copy hex selection (Hex pane only) |

## Settings

SPI clock speed selector (supported: 20, 100, 400, 750 kHz) plus
the prefs-file path display so you know where the persisted state
lives (`~/.config/etch341/prefs.toml` on Linux/macOS,
`%APPDATA%\etch341\prefs.toml` on Windows). Changes save
immediately.

## Activity log

Every operation logs `[HH:MM:SS] message` lines, scrollable, with
the most recent at the bottom. Long lines wrap. Drag the splitter
above the log to resize it; the height persists.
