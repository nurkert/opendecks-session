# OpenDecks Session — Specification

Status: version 2, revised 2026-11-06 on the owner's request: the goal is a
full-featured, open-source DJ toolkit with the feature set of rekordbox 7 at
its highest plan (minus streaming, cloud, video and lighting), arranged the
way rekordbox users know it, in OpenDecks' own visual language. This
document is the goal for implementation. Anything not stated here is out of
scope until it is added to this document. `docs/feature-matrix.md` tracks
every feature against the code.

## 1. Vision

OpenDecks Session is an open-source DJ application written in Rust that lets
a DJ **prepare** a music library and sets on the desktop, keep USB sticks in
sync **without friction**, and **perform** with Pioneer DJ controllers — on
macOS, Linux, and as a standalone unit built from a Raspberry Pi attached to a
controller.

It is an independent product. It is interoperable with the Pioneer DJ USB
export format and with Pioneer DJ controllers. Its **arrangement and
workflow follow rekordbox conventions** (players on top, browser below, tree
on the left, Export-like Prepare and Performance-like Perform modes) so a
rekordbox user is at home immediately; its **visual language is its own**
(DESIGN.md) and it reuses no rekordbox assets. After a one-time migration it
never reads rekordbox directories again.

The feature set is **identical to rekordbox 7's top plan** except for the
fixed exclusions below: library, analysis, cue and grid editing, Explorer playback
straight from folders, stems, all FX, sampler and sequencer, recording, USB
export with automatic sync. Fixed exclusions: streaming services, cloud sync and
accounts (offline-first), video, lighting, DVS.

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

### 4.1a UI architecture (od-ui, rebuilt in v2)

The UI crate is organised by feature, each module owning its view model,
its Slint component and its command handlers:

| Module | Owns |
|---|---|
| `shell` | window, top bar, mode switch, splitters and pane sizes, status band, key bindings |
| `browser` | tree (collection, playlists, sets, smart lists, histories, devices, Explorer), track list, columns, search, filters, tag editing, drag and drop |
| `explorer` | file-system roots, folder listing, import-on-load |
| `player` | Prepare player: waveform, cue / loop / grid editors |
| `decks` | deck view models, waveform lanes, pads and pad modes |
| `mixer` | channels, master, headphones, crossfader |
| `fx` | Beat FX, Color FX, Pad FX, Release FX panels |
| `sampler` | sampler slots, banks, sequencer |
| `recorder` | recording control, recordings list, tracklists |
| `devices` | sticks, sync status, export |
| `preferences` | settings window: audio, controller, view, analysis, keyboard |

A single `AppState` holds the domain state; modules mutate it only through
commands, and a frame tick projects engine state into the deck and mixer
view models. Long work (decode, analysis, separation, sync, export) runs on
workers and reports through channels; the UI thread never blocks.

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

### 5.5 Browser features

- Tree: Collection, Playlists (folders, playlists, smart lists, sets),
  Histories (automatic per session), Explorer (folders on disk), Devices,
  Recordings, Tag list (a staging list), Related tracks and Track
  suggestion panels.
- Track list: configurable and re-orderable columns (incl. preview
  waveform, artwork, rating, colour, comment, My Tag, key, BPM, time,
  date added, play count, bitrate), multi-sort, inline editing of tags,
  rating and colour, drag and drop to playlists / decks / sticks,
  reordering in playlists and sets.
- Search with filters (BPM range, key match, rating, colour, My Tag,
  genre), key and BPM matching against the master deck.
- My Tag: user-defined tag categories and values, usable in smart lists.
- Histories: every performance session is logged as a history list.
- **Explorer:** browse folders on disk and play directly; loading a file
  from Explorer adds it to the collection and analyses it in the
  background, like rekordbox.

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

### 10.1 Decks

- 2 or 4 decks (2 for FLX4), deck layer switching, layouts for 2 and 4
  decks and a browser-maximised layout.
- Tempo fader with ranges (±6/10/16 %, WIDE), **Sync** (BPM and phase,
  beat-aware), master deck, **Quantize**, key lock (Rubber Band), key shift
  and key sync.
- Hot cues A–H with banks, memory cues, loops (manual, auto beat loop,
  halve/double, loop roll, active loop, saved loops), **Slip**, beat jump
  (configurable size), reverse, censor, needle search, auto cue.
- Jog display with position, remaining time, phrase and bar counter.
- Waveforms: three-band colour (RGB and blue variants), overview with phrase
  bar, zoomable scrolling waveform per deck with beat grid, cues and
  phrases; stacked for 2 and 4 decks.
- Pad modes: Hot Cue, Pad FX, Beat Jump, Sampler, Keyboard, Key Shift, Beat
  Loop, Slicer.

### 10.2 Mixer and FX

- Per channel trim / auto gain, 3-band EQ (isolator or classic), Color FX
  knob, fader with curves, CUE; crossfader with assign and curves; master,
  booth, headphone cue / mix / level; meters.
- **Beat FX:** two units, echo, delay, ping pong, reverb, flanger, phaser,
  roll, slip roll, filter, trans, pitch, vinyl brake, helix, spiral; beat
  length, level/depth, channel assign, tap.
- **Color FX:** filter, echo, noise, space, crush, pitch, dub echo, sweep.
- **Pad FX** and **Release FX** (vinyl brake, backspin, echo out).

### 10.3 Stems

- Four parts per track: vocals, drums, bass and instrumental (INST = mix −
  vocals − drums − bass), separated offline by a local ONNX model that is
  downloaded once on demand; nothing leaves the machine.
- Separation runs in the background and is cached per track; a deck shows
  that it is separating until the parts are ready, then the part mutes and
  levels work instantly and click-free; stem FX per part.
- Desktop (Full profile) separates; the Raspberry Pi plays parts that were
  separated on the desktop.

### 10.3a Sampler

- 16 slots in banks, one-shot / loop / gate, per-slot gain, quantised
  triggering, sample sync to the master tempo, a step sequencer with
  pattern recording and saved sequences.

### 10.3b Out of scope

Streaming services, cloud, DVS, lighting, video, Pro DJ Link (later).

### 10.4 Audio I/O

- macOS: CoreAudio.
- Raspberry Pi (Lite profile): ALSA directly on the controller's hardware
  device, no sound server.
- Linux desktop: PipeWire (preferred) or JACK; ALSA as fallback.
- Sample rates 44.1/48 kHz; device buffer configurable.

## 11. Recording

- REC is always one click away in the top bar (and on the controller); a
  recording panel shows level, elapsed time, file size and the tracklist
  as it grows.
- Records the master output of the engine.
- Formats: WAV or FLAC (lossless), optionally MP3 in addition.
- One-button start/stop in the UI and mappable to the controller.
- Optional automatic start/stop by silence detection.
- Automatic tracklist with timestamps (from the engine's "audible track"
  logic): written as `.cue` and as plain text formatted for Mixcloud/SoundCloud.
- Optional split into per-track files at tracklist boundaries.
- Recordings appear in a dedicated library section with their tracklists.

## 12. Design and UX

- **Layout mirrors rekordbox closely** (owner decision 2026-11-06): the
  same regions in the same places and proportions, the same order of
  controls inside decks, mixer, FX and browser, the same panel toggles and
  the same flows (load, browse, record, export). A rekordbox user must
  recognise it instantly. `docs/ux-study/rekordbox-layout.md` is the
  reference; deviations need a documented reason.
- Visual language is OpenDecks' own (DESIGN.md): charcoal ground, bone
  legends, emission-line accents, one colour per deck; no rekordbox assets,
  icons or colours.
- One grid: flat panes separated by shared 1px rules that line up across the
  window; draggable splitters (double-click resets) whose sizes persist.
- Every state (sync status, analysis, separation, recording) is visible at
  a glance; no modal dialogs during performance; keyboard shortcuts for all
  Prepare actions; touch targets ≥ 9 mm in kiosk.
- Accessibility: scalable UI, colour-blind-safe palette, high-contrast
  option, text contrast ≥ 4.5:1.

### 12.1 Beatgrid visibility (owner requirement 2026-11-06)

- Every zoomed waveform (deck lanes, Prepare player) draws the beatgrid as
  full-height lines behind the waveform: beats as thin lines, bar starts
  (downbeats) clearly brighter with a marker at the top, phrase starts
  (every 16 bars or the analysed phrases) strongest. The grid stays legible
  over loud passages and in the colour-blind and high-contrast modes.
- Overviews show bar ticks every 16 bars; the Prepare player shows the bar
  number at each downbeat while editing the grid.
- Checked by renderer tests (line positions within ±1 px of the grid) and
  the view goldens (§16.3).

### 12.2 Preferences and keyboard control (owner requirement 2026-11-06)

- The settings window opens with the platform's native shortcut (⌘, on
  macOS, Ctrl+, on Linux) and from a gear button in the top bar; sections
  as in rekordbox: Audio, View, Analysis, Controller, Keyboard, Recording,
  Advanced. Settings persist; CLI flags only override.
- **The whole application is controllable by keyboard with configurable
  shortcuts.** Every user action is a command in one registry with an id,
  a default binding per platform and optional user bindings; the Keyboard
  page lists all commands (searchable), records a new binding by key
  press, detects conflicts, resets one or all, and imports / exports the
  keymap as TOML.
- Checked by tests: every registered command has a default binding, no
  default conflicts per platform, user overrides round-trip, and the
  Prepare command table is 100 % reachable by keyboard (§16.4).

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

### M5 — Feature parity (v2)

In phases, tracked in `docs/feature-matrix.md`:

1. **P1 everyday:** UI architecture rebuild (§4.1a), Explorer, cue changes
   from performing persisted, track tag editing, playlist/set reordering
   and drag and drop, full cue / loop / grid editor, Beat FX and Color FX
   panels and controller wiring, all mapped controller inputs working,
   Preferences window.
2. **P2 depth:** hot cue banks, pad modes (Pad FX, Keyboard, Key Shift,
   Slicer, Beat Loop), Release FX, sampler panel and sequencer, histories,
   My Tag, related tracks / track suggestion, key shift / key sync,
   recording panel with live tracklist and auto split.
3. **P3 polish:** layouts and panel toggles, waveform options, column
   customisation, matching filters, booth output, censor, saved loops.

Accepted when every non-out-of-scope row of the feature matrix is DONE.

### M6 — Later

OneLibrary export, LAN sync between desktop and Pi, Pro DJ Link, DVS,
more controllers, real-player CDJ validation if not done earlier.

## 16. Verification: parity and superiority (binding acceptance)

The goal of this specification is reached only when the automated suite
below is green on CI (macOS and Linux). "Done" is never asserted by hand.
Everything lives under `tests/parity/` and runs with
`cargo test -p od-parity` (plus the UI suites in `od-ui`).

### 16.1 Feature completeness

- `tests/parity/features.toml` is the machine-readable source of
  `docs/feature-matrix.md`: one entry per rekordbox 7 (top plan) feature
  with `id`, `area`, `scope` (`in` or `out` with the §1 exclusion that
  applies), `status` and `tests` (the names of the automated tests that
  prove it). The markdown matrix is generated from it.
- The completeness test fails when an in-scope feature is not `done`, when a
  `done` feature lists no test, when a listed test does not exist or did
  not run, or when an exclusion is used that §1 does not name.
- New rekordbox features found later are added as entries; the inventory
  is reviewed against the official rekordbox 7 feature list each release.

### 16.2 Functional tests per feature

Every in-scope feature has at least one automated test at the lowest level
that proves its behaviour, and one end-to-end test where a user can reach it:

| Layer | How |
|---|---|
| Engine | offline renders with measured assertions (timing, level, pitch, phase), allocation guard on the audio thread |
| Library / formats / sync | golden files, round trips, three independent parsers for Pioneer exports |
| Analysis | reference tracks with known BPM, key, phrases, loudness (tolerances stated) |
| Controller | recorded MIDI streams → expected actions and LED feedback |
| UI | Slint testing backend: the real window is driven headlessly (clicks, drags, keys) and the resulting engine/library state is asserted |

### 16.3 View completeness against rekordbox

- `tests/parity/views.toml` lists every rekordbox view, layout and panel
  from `docs/ux-study/rekordbox-layout.md` — Performance 2/4 decks
  (horizontal, vertical), Browse, Export 1 player / 2 players / dual /
  simple / full browser, FX panel, Sampler panel, Mix point link, Recording,
  browser sources (Collection, Playlists, Related, Explorer, Devices,
  Histories, Recordings, Tag list, Info panel, sub-browser), Preferences
  sections, context menus — each with its **required elements** (every
  control and readout rekordbox shows there) and their **order** within
  the region.
- The view test opens each view headlessly at 1440×900, 1280×800 and
  800×480 (kiosk) and asserts that every required element exists (by
  accessible label), is visible, unclipped, not overlapping a sibling, in
  the required order and region, and at least 24 px (44 px in kiosk)
  for touch targets.
- Layout invariants are checked on every view: shared rules line up
  across panes (±1 px), no text is elided where the full text fits, text
  contrast ≥ 4.5:1 (large ≥ 3:1), no horizontal scrolling of the window.
- Golden screenshots per view and size are rendered with the software
  renderer and compared to approved images; a visual change needs an
  updated, reviewed golden.

### 16.4 Superiority over rekordbox (measured)

OpenDecks must match rekordbox everywhere and beat it on these, each with
an automated check:

| Criterion | Target / check |
|---|---|
| Steps to common tasks | a flow table (`tests/parity/flows.toml`) states, per task, rekordbox's step count from the UX study and ours; ours must be ≤ and is driven by a UI test (load from Explorer, set and name a hot cue, add to playlist, sync a stick, start a recording, separate stems) |
| Stick sync | edits reach a connected stick automatically, status always visible; no manual export step (rekordbox needs one) |
| Undo | every library edit undoable (test per edit kind) |
| Keyboard | 100 % of Prepare actions reachable by keyboard (generated check over the command table) |
| Responsiveness | cold start, search, track load within §13 targets (benchmarks on CI) |
| Robustness | 1 h soak without dropouts; no modal dialog during performance (UI test asserts no dialog opens in Perform) |
| Accessibility | colour-blind palette, high contrast, scaling 0.75–2.0 without clipping (view test at each scale) |
| Openness | all data in documented open formats; no account, no network for any feature (test with network disabled) |
| Platforms | macOS, Linux, Raspberry Pi kiosk run the same suites |

### 16.5 Definition of done

The goal is reached when, on CI: the completeness test reports zero
in-scope gaps, every functional test passes, every view passes its
completeness, invariant and golden checks at every size, every
superiority check passes, and the hardware-only items of §15 (CDJ, FLX10 /
FLX4, Raspberry Pi) are either verified on hardware or explicitly listed
as awaiting hardware with their simulated tests passing.

## 17. Open items and risks

| Item | Plan |
|---|---|
| No access to CDJs for testing | §8.6 verification chain; real test as soon as possible |
| FLX4 USB IDs, MIDI map and audio layout | Capture with a friend's unit |
| FLX10 audio channel layout and Linux class compliance | Verify with `system_profiler` / `lsusb -v` / `aplay -l` with the device connected |
| rekordbox `master.db` encryption key | Use the publicly documented community approach; importer only |
| Pioneer format writing completeness (`rekordcrate` focuses on reading) | Extend or implement writer in `od-format-pioneer` |
| Rubber Band CPU cost on Pi 4 with 4 decks | Benchmark early in M2; faster engine options in Lite profile |
| GPU waveform rendering on Pi 4 (V3D, GLES 3.1) | Prototype early in M1 to validate Slint + custom rendering |
