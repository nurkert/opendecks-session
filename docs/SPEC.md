# OpenDecks Session — Specification

Status: agreed in the requirements session of 2026-11-06. This document is the
goal for implementation. Anything not stated here is out of scope until it is
added to this document.

## 1. Vision

OpenDecks Session is an open-source DJ application written in Rust that lets
a DJ **prepare** a music library and sets on the desktop, keep USB sticks in
sync **without friction**, and **perform** with Pioneer DJ controllers — on
macOS, Linux, and as a standalone unit built from a Raspberry Pi attached to a
controller.

It is an independent product. It is interoperable with the Pioneer DJ USB
export format and with Pioneer DJ controllers, but it does not reuse
rekordbox assets, layouts, or visual identity, and after a one-time migration
it never reads rekordbox directories again.

### Core problems it solves

1. **USB sticks drift out of date.** Editing a playlist, hot cue, memory cue
   or beatgrid must cascade to every registered stick automatically,
   incrementally and visibly.
2. **Set preparation is clumsy.** Sets are first-class: ordered, annotated,
   with BPM/key/energy flow and compatibility hints.
3. **Recording is fiddly.** One button, automatic tracklist, clean files.
4. **Hardware lock-in.** Runs on Linux and a Raspberry Pi so a controller can
   become a standalone system.

## 2. Ground rules

| Topic | Decision |
|---|---|
| Language | Rust (stable), Cargo workspace |
| License | GPL-3.0-or-later |
| Platforms | macOS 13+ (Apple Silicon + Intel), Linux desktop (x86_64, aarch64), Raspberry Pi 4 and 5 (Raspberry Pi OS Lite 64-bit) |
| Not supported | Windows (explicitly, permanently), 32-bit OS, Raspberry Pi 3 and older |
| UI | Slint; GPU-rendered waveforms embedded in the Slint scene |
| UI languages | English (base), German; i18n from day one |
| Network | Offline-first. No accounts, no cloud, no telemetry. Crash reports are written locally only |
| Repository | `git@github.com:nurkert/opendecks-session.git`, this directory is the workspace root |

### Legal and clean-room rules

- No rekordbox or AlphaTheta code, images, icons, fonts, skins, sounds or
  mapping files are copied into the repository or distributed.
- Reverse engineering (REA, USB captures) is used only to achieve
  interoperability: file formats on the USB stick, the rekordbox library
  import, and the controller protocols.
- Controller mappings are authored independently and verified against the
  real hardware (MIDI capture / MIDI learn). Vendor mapping files may be
  consulted as an interoperability reference but are never redistributed.
- Product names (rekordbox, Pioneer DJ, CDJ, DDJ-FLX10, …) are used only
  descriptively ("compatible with").

## 3. Concepts

### 3.1 Work modes

- **Prepare**: library, analysis, cue and grid editing, playlists, smart
  playlists, sets, USB devices, recordings browser.
- **Perform**: decks, mixer, FX, sampler, recorder, controller operation.

Both modes are part of one application binary. Switching is instant and keeps
state (e.g. a track loaded in Perform stays loaded).

### 3.2 Performance profiles

Orthogonal to the work mode. Detected automatically from the hardware, can
be overridden in settings and with `--profile lite|full`.

| Aspect | Lite (Raspberry Pi 4/5, weak hardware) | Full (desktop) |
|---|---|---|
| Waveform frame rate | 30 fps, reduced detail | 60–120 fps (display rate), full detail |
| Effects | reduced set | full set |
| Keylock | Rubber Band in faster engine settings | highest quality |
| UI animations | off | on |
| Background analysis | throttled, pauses while performing | full speed |
| Audio buffer default | larger (e.g. 256 frames) | small (e.g. 128 frames) |

The audio signal path itself is never degraded by the profile, except the
keylock quality trade-off above.

### 3.3 Kiosk mode (Raspberry Pi)

The Pi boots directly into OpenDecks Session in full screen, Perform mode,
Lite profile, without a desktop environment (Slint `linuxkms` backend). It
is operated mainly through the controller (browse encoder, load buttons,
back/shift functions) and optionally a touchscreen. The library comes from a
USB stick (OpenDecks format) or the Pi's own SSD.

## 4. Architecture

### 4.1 Workspace layout

```
opendecks-session/
├── Cargo.toml                  # workspace
├── crates/
│   ├── od-core/                # domain types, IDs, units (beats, samples, time), errors
│   ├── od-library/             # SQLite library, migrations, undo journal, backups, relocation
│   ├── od-decode/              # symphonia decoding, resampling, file probing, tags
│   ├── od-analysis/            # BPM, beatgrid, key, waveform, phrase, loudness
│   ├── od-import-rekordbox/    # one-time migration: rekordbox library + rekordbox USB stick
│   ├── od-format-pioneer/      # export.pdb + ANLZ (.DAT/.EXT/.2EX) reader and writer
│   ├── od-format-native/       # OpenDecks stick format reader/writer
│   ├── od-devices/             # removable-volume watching, device registry, sync engine
│   ├── od-audio-io/            # CoreAudio, ALSA (direct), PipeWire/JACK backends
│   ├── od-engine/              # real-time engine: decks, mixer, sync, FX, sampler
│   ├── od-recorder/            # recording, encoding, tracklists, splitting
│   ├── od-controller/          # MIDI I/O, mapping format, controller profiles, LED feedback
│   ├── od-i18n/                # translations
│   └── od-ui/                  # Slint UI, waveform renderer, view models
├── apps/
│   ├── opendecks/              # the application binary
│   └── od-cli/                 # headless tool: import, analyze, export, sync, verify
├── mappings/                   # controller mapping files (TOML), see §9
├── packaging/                  # dmg, AppImage, Flatpak, Pi image build
├── tests/fixtures/             # small, license-clean audio and reference sticks
└── docs/
```

Crate boundaries are firm: format crates do not depend on the UI, the engine
does not depend on the library database, and everything below `od-ui` is
usable headless through `od-cli` (which also drives automated tests).

### 4.2 Threads and real-time rules

- **Audio thread**: owned by `od-audio-io`, runs `od-engine`. No allocation,
  no locks, no I/O, no logging in the callback. Communication through
  lock-free SPSC queues (`rtrb`) and atomics. Commands in, meters/positions
  out.
- **Disk/decode workers**: stream and decode audio into pre-allocated ring
  buffers ahead of the playhead; whole-track preload into RAM when memory
  allows (desktop default, Pi when the track fits the budget).
- **Analysis workers**: rayon pool, low priority, cancellable, throttled in
  Lite profile.
- **Sync worker**: one per connected device, serialised writes.
- **UI thread**: Slint event loop; receives state snapshots, never blocks on
  I/O.
- **Controller thread**: MIDI input → mapping → engine commands with
  timestamping; LED output is diffed and rate-limited.

### 4.3 Key dependencies (initial choice, may change with reason)

`slint`, `symphonia`, `rubato` (resampling), Rubber Band (via FFI, GPL),
`rusqlite` (bundled SQLite; SQLCipher feature only inside the importer),
`rekordcrate` (MPL-2.0, reading Pioneer formats; extended or replaced where
writing is needed), `midir`, `cpal` / `alsa`, `pipewire`, `rtrb`, `rayon`,
`notify` + platform volume APIs (DiskArbitration on macOS, udev on Linux),
`fluent` (i18n), `tracing`.

## 5. Library

### 5.1 Storage

- One SQLite database in the platform data directory
  (`~/Library/Application Support/OpenDecks Session/` on macOS,
  `$XDG_DATA_HOME/opendecks-session/` on Linux), WAL mode.
- Analysis blobs (waveforms, beatgrids, phrases) stored in the database or a
  content-addressed sidecar directory next to it, keyed by audio content hash.
- Music files are **referenced** where they are. Optional **Consolidate**
  copies them into a managed music folder. A **Relocate** function finds
  moved/missing files by content hash, size and tags.

### 5.2 Data model (logical)

- `track`: id, file path, content hash, size, format, duration, sample rate,
  tags (title, artist, album, genre, label, year, comment, rating, color),
  date added, play count, analysis version.
- `beatgrid`: track, list of (sample position, BPM, beat number in bar);
  supports variable-tempo grids; `locked` flag.
- `cue`: track, kind (hot cue A–H, memory cue, loop), position, loop length,
  label, color, order.
- `key`: track, key (internal representation), confidence, source
  (analysis/import/manual).
- `waveform`: track, resolution tiers, colour bands (low/mid/high).
- `phrase`: track, sections (intro, verse, chorus, bridge, outro, …).
- `playlist`, `playlist_entry`; `folder` for nesting.
- `smart_playlist`: rule tree (field, operator, value; AND/OR), live
  evaluated.
- `set`: ordered entries with per-transition notes, backup tracks per
  position, planned start time.
- `device`: registered USB stick (volume UUID, label, formats enabled,
  assigned playlists/sets/folders, last sync state).
- `recording`: file, start time, duration, tracklist.
- `history`: played tracks (own performances and imported CDJ history).
- `journal`: undo/redo log of user edits.

### 5.3 Editing, safety and undo

- Every user edit (cues, grids, tags, playlist changes, set changes) goes
  through a command layer that writes an undo journal entry. Undo/redo works
  across the whole library, with a visible history.
- Automatic database backups: on start, before migrations, before imports,
  rotating (last 10 + one per day for 14 days).
- Export/import of the whole library to one archive file (database +
  analysis; optionally including audio).

### 5.4 Supported audio formats

MP3, AAC/M4A, ALAC, FLAC, WAV, AIFF, OGG Vorbis, Opus, decoded with
symphonia. Files that a Pioneer player cannot play (for example Opus) are
still exported to the OpenDecks format but flagged as "not playable on
Pioneer players" in the device view.

## 6. Analysis

- Pure Rust: BPM and beatgrid (including bar/downbeat detection), musical key,
  three-band colour waveform at several resolutions, phrase/section
  detection, loudness (EBU R128) for auto-gain.
- Deterministic and versioned (`analysis_version`). Re-analysis never
  overwrites data that is imported or manually edited unless the user asks.
- Priority of sources: manual > imported > analysed.
- Beatgrid editor: shift, nudge, set downbeat, tap, halve/double, set BPM,
  lock grid.
- Key notation: Camelot (default), Open Key, musical. Display setting only.
- Performance budget: a 6-minute track analysed in ≤ 3 s on an Apple Silicon
  MacBook, ≤ 20 s on a Raspberry Pi 4.

## 7. One-time migration from rekordbox

- Wizard in Prepare mode with two sources:
  1. The local rekordbox library (rekordbox 6/7 `master.db`, SQLCipher
     encrypted, plus its ANLZ files).
  2. A USB stick exported by rekordbox (`export.pdb` + ANLZ).
- Imports: tracks (paths), tags, ratings, colours, beatgrids, hot cues,
  memory cues and loops (with colours and labels), keys, playlists and
  folders, history (optional).
- Works on a **temporary copy** of the source files; never writes to rekordbox
  data. After the import OpenDecks never accesses rekordbox directories
  again; the importer is not called from anywhere else.
- Shows a report: imported counts, missing files, skipped items.

## 8. USB devices and sync

This is the most important part of the product's user experience.

### 8.1 Device model

- A USB stick is **registered** once (by volume UUID) and gets a name.
- Each device has an assigned content: playlists, folders, sets, smart
  playlists (evaluated at sync time).
- Each device has enabled formats: **Pioneer** (legacy `export.pdb` +
  ANLZ) and **OpenDecks** (native). Both are enabled by default.
- Audio files are stored **once** on the stick and referenced by both formats.

### 8.2 Sync behaviour

- Any change that affects a device's content (playlist membership or order,
  cue, grid, tag, colour, track replacement) marks the device **out of date**
  and records which items changed.
- While a registered stick is connected, sync runs automatically in the
  background after a short debounce (about 2 s after the last edit).
  Otherwise sync runs automatically when the stick is next connected.
- Sync is **incremental**: audio files are copied only when new or changed;
  ANLZ/analysis files are rewritten only for changed tracks; `export.pdb` is
  regenerated (small) and written atomically.
- Crash- and unplug-safety: write to temporary files, fsync, atomic rename;
  a journal on the stick allows resuming or rolling back an interrupted sync.
- The device sidebar always shows per stick: connected or not, in sync /
  out of date (with count of pending changes) / syncing (progress) / error,
  free space, formats.
- One-click **safe eject**; eject waits for a running sync to finish.
- Manual controls: "Sync now", "Rebuild stick" (full rewrite), "Remove from
  stick".

### 8.3 Reverse direction (stick → library)

On connect OpenDecks reads back what changed on the stick outside OpenDecks:

- cues created or edited on a Pioneer player, and
- history playlists written by players.

Changes are shown as **proposals** (diff view: per track, old vs new cue) that
the user accepts or rejects; nothing is merged silently.

### 8.4 OpenDecks stick format

- Directory `/OPENDECKS/` with a self-contained SQLite library (subset of the
  main schema), analysis blobs, and a manifest with content hashes and format
  version.
- Readable by OpenDecks on any platform, including the Raspberry Pi kiosk,
  without the main library.

### 8.5 Pioneer format

- Legacy format: `/PIONEER/rekordbox/export.pdb` (and `exportExt.pdb` where
  needed), `/PIONEER/USBANLZ/…/ANLZ0000.DAT/.EXT/.2EX`, artwork,
  `/Contents/…` for audio. Device settings files (`MYSETTING*.DAT`,
  `DEVSETTING.DAT`) are written only with defaults or omitted.
- Target players: everything that reads the legacy format (CDJ-2000NXS2,
  CDJ-3000, XDJ series, standalone systems).
- OneLibrary / Device Library Plus (`exportLibrary.db`) is out of scope until
  M5.

### 8.6 Verification (no CDJ available yet)

1. Golden tests: compare our output structurally with reference sticks that
   rekordbox wrote for the same fixture tracks.
2. Round trip through independent parsers: rekordcrate, crate-digger,
   pyrekordbox must read our sticks without errors and with matching data.
3. rekordbox as a reader: a stick written by OpenDecks must show correct
   tracks, playlists, cues and grids in rekordbox's device view.
4. Real-player testing on CDJs/XDJs is an **open item** to be done as soon as
   hardware is available; it gates the "Pioneer-compatible" claim in
   release notes.

## 9. Controllers

### 9.1 Scope

- First-class support: **Pioneer DDJ-FLX10** (4 decks) and **Pioneer
  DDJ-FLX4** (2 decks).
- Both are USB class compliant (Audio + MIDI) on macOS and are expected to be
  on Linux; no kernel driver is written. FLX10: USB VID `0x2B73`, PID
  `0x0041`. FLX4 IDs to be determined.
- Other controllers are future work, but the layer is vendor-neutral from the
  start.

### 9.2 Controller layer

- `od-controller` maps raw MIDI (notes, CC, 14-bit CC pairs, jog
  rotate/touch, shift layers, deck layers) to **engine actions**.
- Mapping files in TOML (`mappings/<vendor>/<model>.toml`): device matching
  (USB IDs, port names), inputs → actions, outputs (LEDs, VU meters, pad
  colours) ← engine state, per-deck channels, shift handling.
- Optional Rust "device module" per controller for behaviour that cannot be
  expressed declaratively (e.g. jog scaling, vendor USB protocols in M4).
- Built-in **MIDI monitor** and **MIDI learn** for authoring and debugging
  mappings.
- Audio: the controller's audio interface is used for master and headphone
  outputs; the channel layout is detected and can be set per device profile.

### 9.3 Vendor protocol (M4)

The FLX10 settings utility writes device settings with vendor USB control
transfers (jog ring colour, fader start, MIDI channel, virtual deck mode,
etc.), and jog displays/logo upload use a non-MIDI protocol. These are
reverse engineered from a USB capture with the user's controller and REA, and
implemented with `rusb` in the FLX10 device module.

## 10. Perform

### 10.1 Features (v1)

- 4 decks (2 for FLX4), deck layer switching.
- Tempo fader with ranges, **Sync** (BPM and phase, beat-aware), master deck,
  **Quantize**.
- **Keylock** with Rubber Band.
- Hot cues A–H, memory cues, loops (manual, auto-loop in beats, halve/double,
  loop roll), **Slip** mode, beat jump, reverse, censor-style slip reverse.
- Mixer: per-channel trim/auto-gain, 3-band EQ (isolator or classic),
  colour FX filter, channel fader, crossfader with curves, master and
  headphone cue/mix, VU meters.
- Beat FX (small set, e.g. echo, delay, reverb, flanger, phaser, roll,
  filter, trans) and Color FX (filter, echo, noise, space, crush).
- Sampler with banks, triggered from pads.
- Pad modes: Hot Cue, Beat Loop, Beat Jump, Sampler.
- Waveforms: overview + zoomable scrolling waveform per deck, beat markers,
  cue markers, phrase colours; stacked layout for 2 and 4 decks.
- Library browser usable fully from the controller (browse, load, back,
  shift-sort).

### 10.2 Out of scope for v1

Stems, DVS, lighting, streaming services, Pro DJ Link, video.

### 10.3 Audio I/O

- macOS: CoreAudio.
- Raspberry Pi (Lite profile): ALSA directly on the controller's hardware
  device, no sound server.
- Linux desktop: PipeWire (preferred) or JACK; ALSA as fallback.
- Sample rates 44.1/48 kHz; device buffer configurable.

## 11. Recording

- Records the master output of the engine.
- Formats: WAV or FLAC (lossless), optionally MP3 in addition.
- One-button start/stop in the UI and mappable to the controller.
- Optional automatic start/stop by silence detection.
- Automatic tracklist with timestamps (from the engine's "audible track"
  logic): written as `.cue` and as plain text formatted for Mixcloud/SoundCloud.
- Optional split into per-track files at tracklist boundaries.
- Recordings appear in a dedicated library section with their tracklists.

## 12. Design and UX

- Own identity: dark, high contrast for club lighting, calm and precise, one
  accent colour per deck. No resemblance to rekordbox's layout or styling.
- Designed with the design skill before UI implementation: design tokens
  (colour, typography, spacing), component library in Slint, key screens
  (Prepare library, set editor, device sidebar, Perform 2-deck and 4-deck,
  kiosk) in both profiles.
- Interaction principles: every state (sync status, analysis progress,
  recording) is visible at a glance; no modal dialogs during performance;
  keyboard shortcuts for all Prepare actions; touch targets ≥ 9 mm in kiosk.
- Accessibility: scalable UI, colour-blind-safe deck/cue colour palette
  option.

## 13. Performance targets

| Metric | Desktop (Apple Silicon) | Raspberry Pi 4 (Lite) |
|---|---|---|
| Cold start to usable UI | ≤ 2 s | ≤ 6 s (from app start) |
| Library search, 50,000 tracks | ≤ 30 ms | ≤ 150 ms |
| Track load to playable | ≤ 150 ms | ≤ 500 ms |
| Round-trip audio latency target | ≤ 10 ms | ≤ 15 ms |
| Audio dropouts, 1 h, 4 decks + FX + recording | 0 | 0 |
| Cue edit → visible on stick (stick connected) | ≤ 5 s | — |
| UI frame rate during playback | display rate | ≥ 30 fps |

## 14. Packaging and CI

- GitHub Actions: build and test on macOS (arm64, x86_64), Linux (x86_64,
  aarch64).
- Artifacts: `.dmg` (signed and notarised once a Developer ID exists),
  AppImage, Flatpak, Raspberry Pi SD image (Raspberry Pi OS Lite 64-bit +
  kiosk service), `od-cli` binaries.
- Tests: unit tests per crate, format golden tests, engine offline-render
  tests (bit-exact where possible), mapping tests from recorded MIDI streams,
  UI smoke tests.

## 15. Milestones and acceptance criteria

### M1 — Prepare

- Workspace, CI, design system and Prepare UI.
- Library with tags, playlists, folders, smart playlists, sets, undo,
  backups, consolidate, relocate.
- Analysis (BPM, grid, key, waveform, phrase, loudness) and grid/cue editor.
- One-time rekordbox migration (library and stick).
- Device registry and automatic incremental sync in Pioneer and OpenDecks
  formats, reverse-direction proposals, safe eject.

Accepted when: the user's rekordbox library is migrated with cues and grids
intact; a stick written by OpenDecks passes golden tests, the three
independent parsers and rekordbox's device view; editing a hot cue with the
stick connected updates the stick automatically within 5 s with visible
status; all of this runs on macOS and Linux.

### M2 — Perform

- Audio engine, Perform UI (2 and 4 decks), FLX10 and FLX4 mappings with LED
  feedback, recording with tracklists.

Accepted when: a 1-hour 4-deck session on the MacBook with the FLX10 has no
audio dropouts; every FLX10/FLX4 control in the mapping coverage list works
including LEDs; a recording produces a correct `.cue` tracklist.

### M3 — Raspberry Pi standalone

- Lite profile, linuxkms kiosk, controller-only browsing, SD image.

Accepted when: Pi 4 and Pi 5 boot into OpenDecks, play a stick in OpenDecks
format with the FLX10 for 1 hour with 4 decks without dropouts at ≥ 30 fps.

### M4 — Controller vendor protocol

- FLX10 device settings and jog displays via the reverse-engineered USB
  protocol.

### M5 — Later

OneLibrary export, LAN sync between desktop and Pi, stems (desktop only),
more controllers, real-player CDJ validation if not done earlier.

## 16. Open items and risks

| Item | Plan |
|---|---|
| No access to CDJs for testing | §8.6 verification chain; real test as soon as possible |
| FLX4 USB IDs, MIDI map and audio layout | Capture with a friend's unit |
| FLX10 audio channel layout and Linux class compliance | Verify with `system_profiler` / `lsusb -v` / `aplay -l` with the device connected |
| rekordbox `master.db` encryption key | Use the publicly documented community approach; importer only |
| Pioneer format writing completeness (`rekordcrate` focuses on reading) | Extend or implement writer in `od-format-pioneer` |
| Rubber Band CPU cost on Pi 4 with 4 decks | Benchmark early in M2; faster engine options in Lite profile |
| GPU waveform rendering on Pi 4 (V3D, GLES 3.1) | Prototype early in M1 to validate Slint + custom rendering |
