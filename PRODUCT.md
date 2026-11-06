# Product

<!-- impeccable:product-schema 1 -->

## Platform

Native desktop and embedded (macOS 13+, Linux, Raspberry Pi 4/5 kiosk). Not web, not iOS/Android.

## Stack

Rust workspace; UI in Slint (desktop windowing on macOS/Linux, `linuxkms` full-screen backend on the Raspberry Pi). Waveforms rendered by the app itself. Windows is explicitly unsupported.

## Users

Hobby to semi-professional DJs — first the project owner and friends, playing on a Pioneer DDJ-FLX10 and DDJ-FLX4. They prepare music at home on a MacBook and play at bars, clubs, parties and outdoor events, sometimes on club CDJs from a USB stick, sometimes from their own controller, later from a Raspberry Pi built onto the controller. Open source for everyone, but this workflow is the yardstick.

## Product Purpose

Prepare a music library and sets, keep USB sticks in sync without friction, record mixes with one button, and perform with Pioneer DJ controllers on macOS, Linux and a standalone Raspberry Pi. Success: a cue or playlist edit reaches the stick automatically and visibly; sets are prepared deliberately; a performance never drops audio.

## Positioning

An open, cross-platform alternative that treats the USB stick as a live, always-in-sync mirror of the library (automatic incremental sync, visible state per stick, reverse import of cues and history from players), runs on a Raspberry Pi as a standalone unit, and makes set preparation first-class (flow of BPM, key and energy, transition notes, backup tracks).

## Operating Context

- **Prepare:** at home on a MacBook, mouse/trackpad and keyboard, long sessions sorting tracks, setting cues, correcting beatgrids, building sets; USB sticks plugged in and out.
- **Perform:** dark clubs and bars, read from 50–80 cm, operated almost entirely from the controller; must stay readable at parties and outdoors in bright light too.
- **Standalone:** small 7–10" touchscreen attached to the controller, operated by controller (browse encoder, load buttons) plus touch.
- Interoperability: Pioneer DJ legacy USB export (export.pdb + ANLZ) for club players; one-time migration from rekordbox, never accessing rekordbox data afterwards.

## Capabilities and Constraints

See docs/SPEC.md (authoritative). Highlights: two work modes (Prepare, Perform), two performance profiles (Lite for Raspberry Pi, Full for desktop), offline-first with no accounts, cloud or telemetry, English and German UI, GPL-3.0. Stems (vocals, drums, bass, instrumental) are part of v1; no streaming services, DVS, lighting or Pro DJ Link. Terms: "set" (ordered, annotated playlist for a gig), "device"/"stick" (registered USB drive), "hot cue" A–H, "memory cue", "beatgrid", "Camelot key".

## Brand Commitments

Name: OpenDecks Session. Layout and workflow mirror rekordbox closely — same regions, proportions and control order — and the feature set matches rekordbox 7's top plan except for streaming, cloud, video, lighting and DVS (decided by the owner on 2026-11-06). The visual identity stays our own: the emission-line world from DESIGN.md; no rekordbox or AlphaTheta/Pioneer DJ assets, icons, colours or styling; product names only used descriptively. Dark, high contrast, calm and precise; one accent colour per deck.

## Evidence on Hand

No screenshots, users, testimonials or benchmarks exist yet. Do not fabricate any.

## Product Principles

1. The stick is never a mystery: state, pending changes and progress are always visible, and syncing needs no ritual.
2. Nothing interrupts a performance: no modal dialogs, no blocking work, no surprises while playing.
3. Preparation is the craft: tools for sets reward deliberate planning rather than piling up playlists.
4. Same app, any machine: identical concepts on a MacBook and a Raspberry Pi, scaled by profile, not by a second product.
5. The DJ owns the data: open formats, local files, undo for every edit, backups by default.

## Accessibility & Inclusion

Colour-blind safe: deck, cue and key colours are never the only carrier of information, and a colour-blind-safe palette is selectable. UI scaling for small Pi screens and large desktop displays. Readable in both dark clubs and bright daylight (high-contrast option).
