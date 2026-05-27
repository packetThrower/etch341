#!/bin/sh
# Post-install hook for etch341's .deb / .rpm / pacman packages.
# Reloads udev rules and re-triggers currently-attached USB devices
# so the newly-installed 99-ch341a.rules (see that file for what it
# does) applies immediately — users don't have to unplug-replug
# their CH341A or log out after install.
#
# Without this hook, the rule lands on disk but udev hasn't been
# told to re-read its rules.d directory, so /dev/bus/usb/.../<ch341>
# stays at the kernel-default mode (root:root 0600) and etch341
# fails with `PermissionDenied` until the user reboots or runs
# `sudo udevadm control --reload` manually.
#
# Wrapped in a check so chroot / container installs (no /run/udev)
# don't fail noisily; udev will pick the rules up next time it does
# start. Missing or non-executable udevadm is treated the same way
# — not every minimal install has it and it's not an error worth
# aborting the install over.
#
# The same script is wired as both `postinst` (fires on install +
# upgrade) and `postrm` (fires on uninstall + upgrade-replace) so
# any rule version change triggers a re-evaluation either way.

set -e

if command -v udevadm >/dev/null 2>&1 && [ -d /run/udev ]; then
    udevadm control --reload-rules || :
    udevadm trigger --subsystem-match=usb --action=change || :
fi

exit 0
