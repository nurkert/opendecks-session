#!/bin/sh
# Builds "OpenDecks Session.app" and a .dmg in target/dist.
# Signing/notarisation: set CODESIGN_IDENTITY (and run notarytool afterwards).
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release --locked -p opendecks
APP="target/dist/OpenDecks Session.app"
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/opendecks "$APP/Contents/MacOS/opendecks"
cp -R mappings "$APP/Contents/Resources/mappings"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>OpenDecks Session</string>
  <key>CFBundleIdentifier</key><string>org.opendecks.session</string>
  <key>CFBundleExecutable</key><string>opendecks</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Records the controller's audio input when you choose it as a recording source.</string>
</dict></plist>
PLIST
# Bundle Rubber Band next to the binary so the app runs without Homebrew.
for lib in $(otool -L target/release/opendecks | awk '/rubberband/ {print $1}'); do
  cp "$lib" "$APP/Contents/MacOS/"
  install_name_tool -change "$lib" "@executable_path/$(basename "$lib")" "$APP/Contents/MacOS/opendecks"
done
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" "$APP"
fi
hdiutil create -volname "OpenDecks Session" -srcfolder "$APP" -ov -format UDZO "target/dist/OpenDecks-Session-${VERSION}.dmg"
echo "target/dist/OpenDecks-Session-${VERSION}.dmg"
