# OpenDecks Session

Open-source DJ software in Rust for macOS, Linux and the Raspberry Pi.
Prepare your library and sets, keep USB sticks in sync automatically, and
perform with Pioneer DJ controllers (DDJ-FLX10, DDJ-FLX4) — on your laptop or
as a standalone unit built from a Raspberry Pi and your controller.

OpenDecks Session is an independent project and is not affiliated with
AlphaTheta / Pioneer DJ. Product names are used only to describe
compatibility. See [docs/SPEC.md](docs/SPEC.md) for the full specification.

## What it does

- **Library** — referenced music files, fast search, playlists, folders,
  smart playlists with a text rule syntax (`bpm 120-126, key 8A~, genre house`),
  undo/redo for every edit, automatic backups, relocation of moved files.
- **Analysis** — BPM and beatgrid with downbeats, key (Camelot, Open Key or
  musical), three-band waveforms, phrases, loudness; pure Rust, a 6-minute
  track in about 0.3 s on an M1.
- **Sets** — ordered set lists with the flow of tempo, key and energy,
  transition hints (good / caution / clash), transition notes and backups.
- **USB sticks that stay in sync** — register a stick once with the lists
  that belong on it. Every change to a playlist, cue or beatgrid is written
  to the stick automatically and incrementally while it is plugged in, with a
  visible state per stick and one-click eject. Each stick carries the Pioneer
  DJ export (`export.pdb` + ANLZ) for club players and the OpenDecks format
  for OpenDecks itself. Cues set on players and their history are read back
  and offered for review instead of being overwritten.
- **Perform** — 2 or 4 decks, sync and quantize, keylock (Rubber Band), hot
  and memory cues, loops, slip, beat jump, beat FX and colour FX, sampler,
  controller mappings for the DDJ-FLX10 and DDJ-FLX4 with LED feedback.
- **Recording** — one button; WAV with an automatic tracklist as `.cue` and
  as text for Mixcloud/SoundCloud, optional split into one file per track.
- **One-time rekordbox migration** — tracks, cues, beatgrids, keys and
  playlists from your rekordbox library or a rekordbox USB stick. rekordbox
  data is only read from a temporary copy, never changed, and never accessed
  again afterwards.
- **Raspberry Pi standalone** — boots straight into Perform, plays sticks
  prepared on your Mac, controlled entirely from the controller.
- English and German interface; colour-blind-safe state display (state is
  carried by line form, not only by colour).

## Build

Requirements: Rust (stable), Rubber Band, pkg-config.

```sh
# macOS
brew install rubberband pkg-config
# Debian / Ubuntu / Raspberry Pi OS (64-bit)
packaging/linux/install-build-deps.sh

cargo build --release -p opendecks -p od-cli
./target/release/opendecks            # the app
./target/release/opendecks --lang de  # German interface
```

Options: `--library <path>`, `--profile lite|full`, `--kiosk`,
`--scale <factor>`, `--lang en|de`, `--keys camelot|openkey|musical`,
`--colorblind`, `--high-contrast`, `--no-motion`, `--window WxH`.

Packages: `packaging/macos/build-dmg.sh` (.app/.dmg),
`packaging/linux/build-appimage.sh` (AppImage),
`packaging/linux/org.opendecks.Session.yml` (Flatpak),
`packaging/raspberry-pi/` (kiosk bundle, installer and SD-image stage — see
its README).

## First steps

1. **Add music** (or **Import from rekordbox** once).
2. Analysis runs in the background; the status band shows progress.
3. Create playlists and sets; select tracks and press **+** next to a list in
   the sidebar to add them. Click a list's title to rename it.
4. Plug in a USB stick, select the list or folder that should go on it and
   press **Register with …** on the device board. From then on the stick
   follows every change.
5. Switch to **Perform**, double-click a track (or press the browse encoder)
   to load it on a free deck.

Keyboard: ⌘Z / ⌘⇧Z undo/redo, ↑/↓ select, Space preview, 1–8 hot cues,
Delete removes from the list.

## Headless tool

`od-cli` drives the same crates without a window:

```sh
od-cli import ~/Music/House
od-cli analyze
od-cli playlist "Warmup" --search deep
od-cli register /Volumes/STICK --playlist Warmup
od-cli sync /Volumes/STICK
od-cli verify /Volumes/STICK
```

## Architecture

| Crate | Purpose |
|---|---|
| `od-core` | Domain types: beatgrids, cues, keys, waveforms, phrases |
| `od-decode` | Decoding, tags, resampling (symphonia) |
| `od-analysis` | BPM/grid, key, waveform, phrases, loudness |
| `od-library` | SQLite library, undo journal, devices, backups |
| `od-format-pioneer` | `export.pdb` and ANLZ reader/writer |
| `od-format-native` | OpenDecks stick format |
| `od-devices` | Volume detection, incremental sync, background manager |
| `od-import-rekordbox` | One-time migration |
| `od-engine` | Real-time engine: decks, mixer, FX, sampler, record tap |
| `od-audio-io` | CoreAudio / ALSA / JACK / PipeWire output |
| `od-controller` | MIDI, mappings (`mappings/`), LED feedback |
| `od-recorder` | Recording and tracklists |
| `od-i18n` | Translations of messages produced in Rust |
| `od-ui` | Slint interface |
| `apps/opendecks`, `apps/od-cli` | Executables |

Design: [PRODUCT.md](PRODUCT.md) and [DESIGN.md](DESIGN.md).
Controller mapping format: [docs/controller-mappings.md](docs/controller-mappings.md).

## Status and open items

- Real CDJ/XDJ testing of the Pioneer export is still pending (validated
  byte-for-byte against rekordbox output where available and with independent
  parsers: rekordcrate, pyrekordbox).
- Controller mappings follow the published MIDI messages; entries marked
  `verify` in the mapping files need checking on real hardware (the DDJ-FLX4's
  USB IDs are still unknown). `cargo run -p od-controller --example
  midi_monitor` helps with that.
- Vendor USB settings and jog displays of the DDJ-FLX10 (milestone M4) need a
  USB capture with the controller connected.
- OneLibrary (`exportLibrary.db`), LAN sync and stems are planned for M5.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). The bundled Barlow Semi Condensed
font is licensed under the SIL Open Font License (`crates/od-ui/fonts/OFL.txt`).
