#!/bin/sh
# Builds an AppImage in target/dist using linuxdeploy.
set -eu
cd "$(dirname "$0")/../.."
ARCH=$(uname -m)
cargo build --release --locked -p opendecks
APPDIR=target/dist/AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/opendecks"
cp target/release/opendecks "$APPDIR/usr/bin/"
cp -R mappings "$APPDIR/usr/share/opendecks/mappings"
cp packaging/linux/opendecks.desktop "$APPDIR/"
cp packaging/linux/opendecks.svg "$APPDIR/opendecks.svg"
TOOL=target/dist/linuxdeploy-$ARCH.AppImage
[ -x "$TOOL" ] || { curl -fsSL -o "$TOOL" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"; chmod +x "$TOOL"; }
cd target/dist && APPIMAGE_EXTRACT_AND_RUN=1 OUTPUT="OpenDecks-Session-$ARCH.AppImage" "./linuxdeploy-$ARCH.AppImage" --appdir AppDir --output appimage
