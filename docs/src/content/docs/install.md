---
title: Install
description: Install etch341 via the native installer for your platform, or build from source with cargo. macOS .dmg, Windows .msi / NSIS .exe, Linux .deb / .rpm / .pkg.tar.zst / .AppImage.
editUrl: https://github.com/packetThrower/etch341/edit/main/docs/src/content/docs/install.md
---

Every tagged release ships a native installer for each
platform/architecture combination. Head to the [Releases
page](https://github.com/packetThrower/etch341/releases/latest) and
download the artifact that matches your OS + CPU.

System requirements (minimum OS versions, hardware) live in
[Requirements](/etch341/reference/requirements/). Building from
source is covered there too.

## macOS

The path of least friction is the project's Homebrew tap:

```sh
brew install packetThrower/tap/etch341
```

The cask drag-installs the same `.dmg` as the manual flow below
and strips the Gatekeeper quarantine attribute during install so
the first launch doesn't need a right-click → Open dance. Future
releases land via `brew upgrade` on the next 6-hour bump cron, no
re-clicking installers.

Or, the manual flow:

1. Download `etch341-<ver>-arm64-macos.dmg` (Apple Silicon) or
   `etch341-<ver>-amd64-macos.dmg` (Intel) from the Releases page.
2. Open the `.dmg`, drag etch341 to Applications.
3. First launch: right-click → Open (not double-click). Gatekeeper
   shows "etch341 can't be opened because Apple cannot check it for
   malicious software" — that's the standard unsigned-app prompt.
   "Open" once and macOS remembers it forever.

If you'd rather skip the `.dmg` and run the `.app` directly,
`etch341-macOS-<arch>-<ver>.zip` is the bare bundle. Extract and
move into Applications (or anywhere).

No driver setup is needed — macOS leaves the CH341A's vendor
interface alone, and libusb is statically linked into the binary.

## Windows

The CH341A on Windows needs a **one-time WinUSB binding** before
libusb can talk to it. Without it, Windows either enumerates the
device as "Unknown" or claims it with a vendor serial driver — either
way etch341 sees `DeviceNotFound`.

1. Plug in the CH341A.
2. Run [Zadig](https://zadig.akeo.ie/) (≈600 KB, no installer).
3. In Zadig's `Options` menu, enable `List All Devices`.
4. Select the entry with VID `0x1A86` / PID `0x5512`, choose **WinUSB**
   in the driver dropdown, and click `Install Driver`.
5. Install etch341 itself. Easiest path is the project's Scoop bucket:

   ```powershell
   scoop bucket add packetThrower https://github.com/packetThrower/scoop-bucket
   scoop install etch341
   ```

   `scoop update` picks up new releases on the bucket's 6-hour
   bump cron — no clicking through installers. If you'd rather
   click, download `etch341-<ver>-amd64-windows-setup.exe` (or
   `-arm64-windows-setup.exe`) from the Releases page and run
   it. Stable tags also ship a proper `.msi`
   (`etch341-<ver>-<arch>-windows.msi`) that integrates with
   Apps & Features. The portable bare-`.exe` lives in
   `etch341-<ver>-<arch>-windows.zip` for users who want zero
   installer.

Steps 1–4 are needed once per machine.

If `etch341 detect` reports `DeviceNotFound` on Windows after
running it once, the driver binding is usually the cause — re-check
in Zadig that the device is still bound to WinUSB and not to a
vendor driver that took over after an update.

## Linux

Pick the format for your distro and grab it from the Releases page:

| Distro | Artifact |
|---|---|
| Debian / Ubuntu / Mint | `etch341-<ver>-<arch>-linux.deb` |
| Fedora / openSUSE / RHEL | `etch341-<ver>-1.<arch>.rpm` |
| Arch / Manjaro | `etch341-<ver>-1-<arch>.pkg.tar.zst` |
| Any (universal) | `etch341-<ver>-<arch>-linux.AppImage` |

The `.deb` / `.rpm` / `.pkg.tar.zst` install paths drop the udev
rule into `/usr/lib/udev/rules.d/99-ch341a.rules` automatically. For
the AppImage, run this once:

```sh
sudo cp 99-ch341a.rules /etc/udev/rules.d/
sudo udevadm control --reload
# then unplug + replug the CH341A
```

(The udev rule itself is in the repo at
`platform/udev/99-ch341a.rules`, and inside any of the package
installers.)

Without the udev rule, unprivileged users hit `PermissionDenied`
opening the CH341A.

## From source (cargo)

```sh
git clone https://github.com/packetThrower/etch341.git
cd etch341
cargo install --path .                              # GUI + CLI
cargo install --path . --no-default-features        # CLI-only, smaller binary
```

Linux from source needs the GPUI build-time deps:

```sh
sudo apt install \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libwayland-dev libx11-dev libxcb1-dev libxcb-randr0-dev \
  libxcb-xkb-dev libxcb-cursor-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxcb-render0-dev \
  libfontconfig1-dev libfreetype-dev pkg-config
```

libusb is statically linked into the binary via `rusb`'s `vendored`
feature, so there's no `libusb-1.0-0-dev` requirement at build or
runtime.
