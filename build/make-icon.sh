#!/usr/bin/env bash
# Regenerate the icon asset set from build/appicon.svg, the source
# of truth (a top-down SOIC-8 chip on a rounded navy square —
# matches the visual language of Baudrun and PortFinder).
#
# Requires:
#   - rsvg-convert (from librsvg) — rasterize SVG → PNG at any size
#   - magick (ImageMagick 7)      — multi-resolution .ico for Windows
#   - iconutil (macOS, built-in)  — .iconset → .icns for macOS bundles
#
# Outputs:
#   build/appicon.png              — 1024×1024 canonical raster
#   build/windows/icon.ico         — multi-res icon, 16…256 px
#   resources/icons/icon.svg       — source SVG copied to resources/
#   resources/icons/icon.png       — 1024×1024, used by Linux bundles
#   resources/icons/{32,64,128}x128.png + 128x128@2x.png
#                                  — per-size PNGs cargo-packager
#                                    drops into .deb / .AppImage
#   resources/icons/icon.icns      — macOS bundle icon, built via
#                                    iconutil from a temp .iconset
#   docs/public/favicon.svg        — flat SVG copy served by the
#                                    Astro+Starlight docs site as its
#                                    <link rel="icon">

set -euo pipefail
cd "$(dirname "$0")"

# Canonical 1024×1024 PNG used as the source for every raster below.
rsvg-convert -w 1024 -h 1024 appicon.svg -o appicon.png
echo "Wrote $(pwd)/appicon.png"

mkdir -p windows
magick appicon.png -define icon:auto-resize=256,128,64,48,32,16 windows/icon.ico
echo "Wrote $(pwd)/windows/icon.ico"

# resources/icons/ holds the cross-platform set that cargo-packager
# consumes at build time (icons = [...] in [package.metadata.packager]).
RES=../resources/icons
mkdir -p "$RES"

cp appicon.svg "$RES/icon.svg"
cp appicon.png "$RES/icon.png"

rsvg-convert -w 32  -h 32  appicon.svg -o "$RES/32x32.png"
rsvg-convert -w 64  -h 64  appicon.svg -o "$RES/64x64.png"
rsvg-convert -w 128 -h 128 appicon.svg -o "$RES/128x128.png"
rsvg-convert -w 256 -h 256 appicon.svg -o "$RES/128x128@2x.png"
echo "Wrote $RES/{32,64,128}x128.png + 128x128@2x.png"

# macOS .icns: iconutil wants a directory with the full Apple-spec
# set (16/32/128/256/512 each at 1× and 2×). Build the iconset in a
# tempdir, hand it to iconutil, clean up.
ICONSET=$(mktemp -d)/icon.iconset
mkdir -p "$ICONSET"
for sz in 16 32 64 128 256 512; do
  rsvg-convert -w $sz       -h $sz       appicon.svg -o "$ICONSET/icon_${sz}x${sz}.png"
  rsvg-convert -w $((sz*2)) -h $((sz*2)) appicon.svg -o "$ICONSET/icon_${sz}x${sz}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$RES/icon.icns"
rm -rf "$(dirname "$ICONSET")"
echo "Wrote $RES/icon.icns"

# Website favicon — flat SVG so the browser renders it at any size.
# Lives in the Astro `public/` dir so Starlight's `favicon: '/favicon.svg'`
# config picks it up.
mkdir -p ../docs/public
cp appicon.svg ../docs/public/favicon.svg
echo "Wrote ../docs/public/favicon.svg"
