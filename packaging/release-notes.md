OpenDecks Session 0.1.0 — the first regular release of the open-source DJ software for macOS, Linux and the Raspberry Pi.

## Downloads

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `OpenDecks-Session-*-macos-arm64.dmg` |
| macOS (Intel) | `OpenDecks-Session-*-macos-x86_64.dmg` |
| Debian 12+, Ubuntu 22.04+ (x86_64) | `opendecks-session_*_amd64.deb` |
| Debian 12+, Ubuntu 22.04+, Raspberry Pi OS 64-bit (arm64) | `opendecks-session_*_arm64.deb` |
| Any Linux | `OpenDecks-Session-*.AppImage`, `opendecks-session-*-linux-*.tar.gz` |
| Raspberry Pi standalone (kiosk) | `opendecks-pi-*.tar.gz` — see `packaging/raspberry-pi/README.md` |

`SHA256SUMS.txt` lists the checksums of every file.

macOS: the app is not notarised yet. On first start, right-click the app → **Open**, or run
`xattr -dr com.apple.quarantine "/Applications/OpenDecks Session.app"`.

## What's in it

- **Library and preparation:** analysis (BPM and beatgrid, key, 3-band waveforms, phrases, loudness), playlists, smart playlists, sets with energy/key flow and transition notes, My Tag, related tracks, histories, duplicates, cover art, an Explorer for folders on disk, a grid and phrase editor, undo for every edit
- **USB sticks that stay in sync:** playlists, hot cues and memory cues reach a connected stick automatically, in a format compatible with common club players and in the open OpenDecks format; cues and history written by players are offered for review
- **Perform:** 2 or 4 decks (horizontal, vertical, browse layouts), sync, keylock, stems (part mutes, part isolation, Stem FX), Beat FX and Colour FX, sampler with sequencer, Mix Point Link, pad modes, CDJ practice mode
- **Recording:** one button, automatic tracklist, splitting
- **Lighting:** DMX over Art-Net, sACN and USB-DMX with automatic, easy and pro modes, cue stack and cues stored with tracks, in its own window if you like
- **Controllers:** mappings for the Pioneer DJ DDJ-FLX10 and DDJ-FLX4, MIDI learn and a MIDI monitor
- **MCP server** (`opendecks-mcp`): prepare sets and run the light show together with an AI assistant
- **Migration:** a one-time import of libraries and sticks from rekordbox (compatible import; cues, grids, playlists, histories and cover art)
- English and German, dark / light theme, configurable keyboard shortcuts, UI scaling

Pioneer DJ, rekordbox and DDJ are trademarks of their owners. OpenDecks Session is an independent project and is not affiliated with or endorsed by them; it only aims to be compatible.

## Known limitations

See [docs/verification.md](https://github.com/nurkert/opendecks-session/blob/main/docs/verification.md). Still to be verified on hardware: playback of exported sticks on real players, the controller mappings, the DDJ-FLX10's built-in DMX port and jog displays, and the Raspberry Pi kiosk.
