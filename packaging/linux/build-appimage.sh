#!/bin/sh
# Builds an AppImage in target/dist using linuxdeploy (expects a release build).
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ARCH=$(uname -m)
[ -x target/release/opendecks ] || cargo build --release --locked -p opendecks -p od-cli
APPDIR=target/dist/AppDir
rm -rf "$APPDIR" && mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/opendecks"
cp target/release/opendecks target/release/od-cli "$APPDIR/usr/bin/"
cp -R mappings "$APPDIR/usr/share/opendecks/mappings"
# The tool lives outside target/dist so it never ends up in a release.
TOOL=target/tools/linuxdeploy-$ARCH.AppImage
mkdir -p target/tools
[ -x "$TOOL" ] || { curl -fsSL -o "$TOOL" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$ARCH.AppImage"; chmod +x "$TOOL"; }
cd target/dist
APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 OUTPUT="OpenDecks-Session-${VERSION}-${ARCH}.AppImage" "../tools/linuxdeploy-$ARCH.AppImage" \
  --appdir AppDir --executable AppDir/usr/bin/opendecks \
  --desktop-file ../../packaging/linux/opendecks.desktop --icon-file ../../packaging/linux/opendecks.svg \
  --output appimage
rm -rf AppDir
