First public preview of OpenDecks Session — open-source DJ software for macOS, Linux and the Raspberry Pi.

## Downloads

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `OpenDecks-Session-*-macos-arm64.dmg` |
| macOS (Intel) | `OpenDecks-Session-*-macos-x86_64.dmg` |
| Debian 12+, Ubuntu 22.04+ (x86_64) | `opendecks-session_*_amd64.deb` |
| Debian 12+, Ubuntu 22.04+, Raspberry Pi OS 64-bit (arm64) | `opendecks-session_*_arm64.deb` |
| Any Linux | `OpenDecks-Session-*.AppImage`, `opendecks-session-*-linux-*.tar.gz` |
| Raspberry Pi standalone (kiosk) | `opendecks-pi-*.tar.gz` — see `packaging/raspberry-pi/README.md` |

macOS: the app is not notarised yet. On first start, right-click the app → **Open**, or run
`xattr -dr com.apple.quarantine "/Applications/OpenDecks Session.app"`.

## What's in it

- Library with analysis (BPM/beatgrid, key, waveforms, phrases), playlists, smart playlists and sets with flow and transition hints
- USB sticks that stay in sync automatically, in the Pioneer DJ export format and the OpenDecks format; cues and history written by players are offered for review
- Perform with 2 or 4 decks, sync, keylock, FX, sampler; mappings for DDJ-FLX10 and DDJ-FLX4
- One-button recording with automatic tracklist
- One-time rekordbox migration
- English and German

## Known limitations

See [docs/verification.md](https://github.com/nurkert/opendecks-session/blob/main/docs/verification.md): playback on real CDJs, the controller mappings and the Raspberry Pi kiosk still need testing on hardware.
