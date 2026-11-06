# Verification status

State of the acceptance criteria in [SPEC.md](SPEC.md) §15, with the evidence
behind each verified item. Date: 2026-11-06.

## Verified

| Criterion | Evidence |
|---|---|
| Builds and tests on macOS (Apple Silicon, Intel) and Linux (x86_64, aarch64) | GitHub Actions CI, all jobs green; 216 unit/integration tests |
| Raspberry Pi kiosk binary builds (aarch64, `linuxkms`) | CI job `kiosk` |
| Headless chain on aarch64: import → Lite analysis → set → register stick → sync → verify → no-op resync, tempos within 0.1 BPM | CI step `packaging/ci/headless-e2e.sh` on `ubuntu-24.04-arm` |
| M2: one hour, 4 decks with keylock, sync, tempo changes, 2 beat FX, colour FX and recording without dropouts | `cargo run --release -p od-audio-io --example soak -- 3600 128 full …` on an M1 Max, CoreAudio, 128-frame buffer: **3600 s, 1,350,017 callbacks, 0 xruns, 0 overruns**, peak load 92.4 % (keylock priming), typical 13–40 % |
| M1: stick sync on real file systems | Synced to FAT32 and exFAT volumes: full sync 17 tracks / 670 MB in 2.6 s, no-op resync 0.1 s, both formats read back by `od-cli verify` (this found and fixed a SQLite locking bug on FAT) |
| M1: cue edit reaches a connected stick automatically | `od-devices` manager test `registers_and_auto_syncs` (edit in the app's connection → background sync → stick library has the new cue) |
| M1: Pioneer export structure | Empty export byte-identical to rekordbox's; 600-track export parsed identically by `rekordcrate`; ANLZ files parsed by `rekordcrate` and `pyrekordbox`; a real 3,886-track rekordbox export parses |
| M1: rekordbox migration with cues and grids | Read-only run against the owner's rekordbox 7 library into a scratch library: 576 tracks, 1,646 cues, 556 grids, 549 keys, 79 playlists, no errors |
| M3 layout on 7" and 10" panels | Kiosk captured at 800×480 and 1280×800 (design review, two rounds) |

## Requires hardware (not verifiable on this machine)

| Criterion | What is needed |
|---|---|
| Pioneer sticks on real CDJ/XDJ players (SPEC §8.6 step 4; gates any "Pioneer-compatible" claim) | Access to a CDJ-2000NXS2 / CDJ-3000 / XDJ |
| rekordbox device view shows an OpenDecks stick (§8.6 step 3) | A manual look in rekordbox with a registered stick plugged in |
| M2: every FLX10/FLX4 control and LED | The controllers connected; entries marked `verify` in `mappings/pioneer/*.toml`; `cargo run -p od-controller --example midi_monitor -- --mapping …` |
| M3: Pi 4 and Pi 5 boot into OpenDecks and play a stick for an hour at ≥ 30 fps | The Pi; `packaging/raspberry-pi/README.md`; the same soak with `-- 3600 256 lite` |
| M4: FLX10 vendor settings and jog displays | FLX10 connected; settings protocol is implemented from static analysis (`docs/ddj-flx10-usb-protocol.md`), jog displays need a USB capture |
