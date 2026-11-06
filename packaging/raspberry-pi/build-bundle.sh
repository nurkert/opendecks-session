#!/bin/sh
# Builds the Raspberry Pi bundle (run on aarch64 Linux, e.g. a Pi 5 or an
# arm64 CI runner): target/dist/opendecks-pi-<version>.tar.gz
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release --locked -p opendecks -p od-cli -p od-mcp --features opendecks/kiosk
OUT=target/dist/opendecks-pi-$VERSION
rm -rf "$OUT" && mkdir -p "$OUT"
cp target/release/opendecks target/release/od-cli target/release/opendecks-mcp "$OUT/"
cp -R mappings "$OUT/mappings"
cp packaging/raspberry-pi/install.sh packaging/raspberry-pi/opendecks-kiosk.service \
   packaging/raspberry-pi/99-opendecks-usb.rules packaging/raspberry-pi/50-opendecks-udisks.rules "$OUT/"
tar -C target/dist -czf "$OUT.tar.gz" "opendecks-pi-$VERSION"
echo "$OUT.tar.gz"
