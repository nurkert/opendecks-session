#!/bin/sh
# Builds the Linux release artifacts for the current architecture into
# target/dist: .deb, AppImage and a portable tar.gz.
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
DEB_ARCH=$(dpkg --print-architecture)
cargo build --release --locked -p opendecks -p od-cli
mkdir -p target/dist

# .deb
command -v cargo-deb >/dev/null || cargo install --locked cargo-deb
cargo deb --no-build -p opendecks -o "target/dist/opendecks-session_${VERSION}_${DEB_ARCH}.deb"

# Portable tar.gz
T="target/dist/opendecks-session-${VERSION}-linux-$(uname -m)"
rm -rf "$T" && mkdir -p "$T"
cp target/release/opendecks target/release/od-cli "$T/"
cp -R mappings "$T/mappings"
cp README.md LICENSE packaging/linux/opendecks.desktop packaging/assets/icon.svg "$T/"
tar -C target/dist -czf "$T.tar.gz" "$(basename "$T")"

# AppImage
packaging/linux/build-appimage.sh
ls -la target/dist
