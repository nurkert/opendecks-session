# Feature gap matrix (rekordbox 7 workflow vs. OpenDecks Session)

Snapshot: 2026-11-06, from a read of the source tree (no build run). The
reference list is the rekordbox 7 feature set relevant to a DJ who prepares in
EXPORT mode and plays in PERFORMANCE mode with a DDJ-FLX10 / DDJ-FLX4. Only
feature names are taken from rekordbox; behaviour, layout and naming in
OpenDecks follow [SPEC.md](SPEC.md) and [DESIGN.md](../DESIGN.md).

Note: `od-ui/src/render.rs::detail` already takes a `phrases` argument while
its two callers (`perform.rs::frame`, `app.rs::refresh_detail`) still pass the
old argument list, so phrase display is mid-change at the time of writing.

## Legend

| Status | Meaning |
|---|---|
| DONE | Works end to end, including UI and/or controller |
| ENGINE-ONLY | Backend (engine, library, format) exists; no UI and no controller path, or the controller action is mapped but dropped in `od-ui/src/perform.rs::handle_action` |
| PARTIAL | Usable, but an expected part is missing (noted) |
| MISSING | Nothing in the code |
| OUT-OF-SCOPE | Excluded by SPEC (reason given) |

Priority (gaps only): **P1** = a rekordbox user hits it in the first session
and the app feels broken or foreign without it; **P2** = expected, regular
use; **P3** = niche, power-user or later.

Short paths used below: `perform.rs` = `crates/od-ui/src/perform.rs`,
`app.rs` = `crates/od-ui/src/app.rs`, `command.rs` =
`crates/od-engine/src/command.rs`, `action.rs` =
`crates/od-controller/src/action.rs`, `flx10.toml` =
`mappings/pioneer/ddj-flx10.toml`.

---

## 1. Browser / collection

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Collection table with sortable columns | DONE | `ui/table.slint` header cells; `app.rs::sort_key` (title, artist, genre, BPM, key, time, rating, date added) | |
| Column choice (album, label, comment, colour, date added, play count, bitrate, My Tag, …) | MISSING | Fixed column set in `table.slint`; `TrackRow` has `album` but no header for it | P2 |
| Artwork in browser / deck | PARTIAL | Stored (`schema.sql` `artwork`, `tracks.rs::artwork`) and exported to sticks (`pdb/rows.rs`); not shown in the UI | P3 |
| Tree: folders, playlists, smart lists, sets | DONE | `nodes.rs::create_node/rename_node/delete_node`; `app.rs` `on_create_node`, `on_rename_node`, `on_delete_node` | |
| Reorder tree nodes (drag) | ENGINE-ONLY | `nodes.rs::move_node`; no UI | P2 |
| Add tracks to playlist | DONE | `app.rs` `on_add_to_node` | |
| Reorder playlist / set entries | ENGINE-ONLY | `nodes.rs::move_entries`; not called from `od-ui` | **P1** |
| Drag and drop (table → tree, Finder → library) | MISSING | No drag/drop handlers in `ui/*.slint` | **P1** (with reorder) |
| Search | DONE | `tracks.rs::search`; `app.rs` `on_search` | |
| Filter (BPM range, key, rating, My Tag, relative to master deck) | PARTIAL | Smart-list rules can express it (`smart.rs` `Field`/`Op`, incl. `HarmonicWith`); no quick filter bar in the browser | P2 |
| Rating (stars) | PARTIAL | Shown and sortable (`TrackRow.rating`); not editable (`tracks.rs::set_tags` is unused by the UI) | **P1** |
| Track colour | ENGINE-ONLY | `TrackTags.color` (`od-core/src/track.rs`), imported and exported; no column, no editor | **P1** |
| Comments | ENGINE-ONLY | `TrackTags.comment`; smart-rule field `Comment`; no column, no editor | **P1** |
| Edit tags (title, artist, album, genre, label, year …) | ENGINE-ONLY | `tracks.rs::set_tags` (journalled); no track-info editor | **P1** |
| My Tag | MISSING | Importer skips My Tag smart conditions (`od-import-rekordbox/src/tests.rs`); no model | P2 |
| Tag list / palette (holding area) | MISSING | — | P3 |
| Related tracks | ENGINE-ONLY | `sets.rs::suggest_next`; FLX10 `browse.related` is mapped but dropped in `handle_action` | P2 |
| Track suggestion / harmonic matching | PARTIAL | Key-distance column vs. selected track (`app.rs` `key_distance`), set flow fit (`sets.rs::transition`); no suggestion list | P2 |
| Intelligent (smart) playlists | DONE | `od-library/src/smart.rs`, text rule editor `od-ui/src/smart_text.rs`, `on_set_smart_rule` | |
| Histories: player histories from sticks | DONE | `od-devices/src/pioneer.rs` history proposals → `app.rs::resolve_proposal` creates a "History" folder | |
| Histories: own play history | PARTIAL | `tracks.rs::record_play` is only called while recording (`perform.rs::frame`); no history view | P2 |
| Track preview (Prepare) | DONE | `perform.rs::toggle_preview` on a preview deck; Space key | |
| Loaded-on-deck indicator | DONE | `TrackRow.deck` | |
| Duplicate track check | MISSING | — | P3 |
| Import folder / files | DONE | `app.rs` `on_import_folder`, `od-cli import` | |
| rekordbox library + stick migration | DONE | `od-import-rekordbox`, `od-ui/src/import.rs` | |
| rekordbox XML / iTunes import-export | MISSING | — | P3 |
| Missing files / relocate | DONE | `relocate.rs::check_missing/find_relocations`; `on_find_moved_files` | |
| Consolidate | ENGINE-ONLY | `relocate.rs::consolidate`; no UI | P3 |
| Undo/redo with history | DONE (beyond rekordbox) | `journal.rs`; Cmd-Z / Shift-Cmd-Z in `app.rs::key` | |
| Sets with transition notes and flow view | DONE (own) | `sets.rs::set_flow`, `on_set_transition_note`, `ui/flow.slint` | |
| Set backup tracks / planned start | ENGINE-ONLY | `nodes.rs::set_entry_backups/set_set_meta` | P2 |
| Keyboard shortcuts for Prepare | PARTIAL | `app.rs::key`: undo/redo, delete, arrows, Space, 1–8; SPEC §12 wants all Prepare actions | P2 |
| Browse from controller | PARTIAL | Rotate / press / back handled (`handle_action` `BrowseAction`); tree navigation, view toggle, shift-sort and related dropped | P2 |

## 2. Analysis

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| BPM + beatgrid incl. downbeats | DONE | `od-analysis/src/tempo.rs::detect_beatgrid`; background worker `od-ui/src/workers.rs` | |
| Variable-tempo grid | PARTIAL | Model supports it (`od-core/src/beatgrid.rs`); editor only rebuilds constant grids (`on_grid_scale`) | P3 |
| Grid editor: shift, ×2/÷2, set downbeat, lock | DONE | `app.rs` `on_grid_shift/on_grid_scale/on_grid_downbeat/on_grid_lock` | |
| Grid editor: tap tempo, type BPM | MISSING | Required by SPEC §6 | **P1** |
| BPM analysis range setting | MISSING | — | P3 |
| Key detection, notation Camelot/Open Key/musical | DONE | `od-analysis/src/key.rs`; `--keys` flag, `KeyNotation` | |
| Phrase analysis | PARTIAL | Analysed (`od-analysis/src/phrase.rs`), exported (`PSSI` in `anlz/build.rs`); display being wired (`render.rs::phrase_strip`, see note) | P2 |
| Waveform: own 3-band colour | DONE | `od-analysis/src/waveform.rs`, `render.rs` (+ colour-blind palette) | |
| Waveform style options (RGB / blue / 3-band) | MISSING | One style by design; stick export still writes `PWAV/PWV2–PWV7` for players | P3 |
| Vocal position | MISSING | Could be derived from the vocal stem | P3 |
| Loudness + auto gain | DONE | `od-analysis/src/loudness.rs`; gain in `perform.rs::load_track`, engine default on (`mixer.rs` `auto_gain: true`); no toggle in UI | |
| Re-analyse selected, never overwrite manual/imported | DONE | `on_analyse_selected`; `DataSource` priority in `tracks.rs::store_analysis` | |

## 3. Cues and loops

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| 8 hot cues A–H (Prepare) | DONE | `app.rs` `on_hot_cue/on_remove_hot_cue`; keys 1–8 | |
| 8 hot cues A–H (Perform, pads/UI) | PARTIAL | `DeckCommand::HotCue`; set/delete live only in the engine, **not saved to the library** (no `set_cues` call in `perform.rs`), so never reaches the stick | **P1** |
| Hot cue banks / hot cue bank lists | MISSING | — | P3 |
| Memory cues: add (Prepare) | DONE | `on_add_memory_cue` | |
| Memory cues: delete, move (Prepare) | MISSING | No callback | **P1** |
| Memory cues: call prev/next (Perform) | DONE | `MemoryCueJump` via controller | |
| Memory cue set/delete from controller | ENGINE-ONLY | Mapped (`deck.memory_cue_set/delete` in `flx10.toml`), dropped in `handle_action` | **P1** |
| Saved loops (memory/hot-cue loops) | PARTIAL | Model, import, export and playback exist (`Cue.loop_end`); cannot be created in Prepare; loop hot cues made in Perform are not persisted | **P1** |
| Active loop (auto-engaged saved loop) | MISSING | — | P3 |
| Cue colours | PARTIAL | `Cue.color` imported/exported; not drawn (`CueMark` has no colour), not editable | **P1** |
| Cue comments / labels | PARTIAL | Shown on pads (`HotCueSlot.label`); not editable | **P1** |
| Auto cue on load (first memory cue / first beat) | DONE | `DeckCommand::Load` behaviour (`command.rs` doc, `deck.rs`) | |
| Cues set on players → library | DONE (beyond rekordbox) | `od-devices/src/pioneer.rs` proposals, `on_accept_proposal` | |

## 4. Deck / player

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| 2 / 4 decks, deck layers | DONE | `on_set_deck_count`, FLX10 auto 4 decks (`perform.rs::frame`), `DeckSelect` in `od-controller/src/session.rs` | |
| Play / Cue (CDJ cue logic) | DONE | `DeckCommand::Cue`; UI + controller | |
| Jog: vinyl scratch, bend, search; vinyl mode toggle | DONE | `DeckCommand::JogTurn/JogTouch/SetJogMode`, `handle_action` | |
| Jog display (FLX10 screens) | MISSING | Needs the non-MIDI protocol (SPEC M4, `docs/ddj-flx10-usb-protocol.md`) | P3 (M4) |
| Tempo fader, ranges ±6/10/16/WIDE, reset | DONE | `TempoRange`; controller cycles range; UI shows it but has no range button | |
| Master tempo / key lock (Rubber Band) | DONE | `od-engine/src/keylock`, UI toggle + controller | |
| Key shift (±semitones) | ENGINE-ONLY | `DeckCommand::SetKeyShift`; only `KeyReset` reaches it | P2 |
| Key sync | MISSING | Mapped (`deck.key_sync`); no engine command | P2 |
| Beat sync + master deck | DONE | `SetSync`, `SetMasterDeck`; UI + controller | |
| Quantize | DONE | `Command::SetQuantize`; UI + controller | |
| Slip | DONE | `SetSlip`; UI + controller | |
| Reverse | DONE | `SetReverse` via controller; no UI button | |
| Censor (slip reverse) | DONE | `DeckAction::Censor` in `handle_action` | |
| Beat jump | DONE | `DeckCommand::BeatJump`; UI ±4, controller, pad mode | |
| Beat jump size setting | MISSING | Fixed ±4 in UI, pad sizes fixed in `handle_action` | P3 |
| Manual loop in/out, reloop/exit, ½ / 2× | DONE | `LoopIn/LoopOut/Reloop/LoopExit/LoopHalve/LoopDouble` | |
| Auto beat loop (1/32 … 32) | DONE | `DeckCommand::AutoLoop`; UI size stepper | |
| Loop roll | DONE | `DeckCommand::LoopRoll` via controller | |
| Needle search | DONE | `NeedleSearch`; overview click + FLX touch strip | |
| Search (FF/REW buttons) | MISSING | Mapped (`deck.search`); no engine command, dropped | **P1** (controller coverage) |
| Tap BPM | MISSING | Mapped (`deck.tap_bpm`), dropped | **P1** (with grid tap) |
| Pad mode: Hot Cue | DONE | `PadMode::HotCue` in `handle_action` | |
| Pad mode: Beat Loop | DONE | `PadMode::BeatLoop` → `AutoLoop` | |
| Pad mode: Beat Jump | DONE | `PadMode::BeatJump` | |
| Pad mode: Sampler | PARTIAL | Triggers slots, but no way to load samples (see §8) | P2 |
| Pad mode: Pad FX 1/2 | MISSING | `PadMode::PadFx1/2` exist in `action.rs`; no effect logic | P2 |
| Pad mode: Keyboard | MISSING | `PadMode::Keyboard` selectable; no logic (needs key shift) | P3 |
| Pad mode: Key Shift | ENGINE-ONLY | `PadMode::KeyShift` selectable; engine `SetKeyShift` exists; pads dropped | P2 |
| Pad mode: Slicer | MISSING | Not in `PadMode` | P3 |
| Pad LED feedback per mode | PARTIAL | Only hot-cue pad LEDs (`perform.rs::feedback`) | P2 |
| Waveform zoom in Perform | MISSING | Fixed `DETAIL_SPAN` 8 s (`perform.rs`) | P2 |
| Waveform overview + scrolling detail, cue markers, beat ticks | DONE | `render.rs::overview/detail`, `ui/plate.slint` | |

## 5. Mixer

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Trim | DONE | `ChannelCommand::SetTrim`; UI knob + controller | |
| 3-band EQ (isolator) | DONE | `dsp/eq.rs` default `Isolator`; UI + controller | |
| EQ mode classic/isolator | ENGINE-ONLY | `ChannelCommand::SetEqMode`; no setting | P2 |
| Colour FX knob (filter) | DONE | `SetColorFxAmount`; UI + controller (default kind = filter) | |
| Channel fader | DONE | UI + controller | |
| Channel fader curve | ENGINE-ONLY | `FaderCurve` / `SetFaderCurve`; no setting | P2 |
| Crossfader | DONE | UI + controller | |
| Crossfader curve | ENGINE-ONLY | `CrossfaderCurve`; no setting | P2 |
| Crossfader assign A/THRU/B | DONE | Controller (`MixerAction::CrossfaderAssign`); no UI control | |
| Headphone cue, mix, level | DONE | `SetCue`, `SetHeadphoneMix/Level`; UI + controller | |
| Master level, VU meters, limiter | DONE | `SetMasterLevel`, meters in `ChannelView`, limiter on by default | |
| Master cue | MISSING | Mapped (`master_cue`), no engine function | P2 |
| Booth level / booth output | MISSING | Mapped (`booth_level`), no engine bus | P2 |
| Smart CFX / Smart Fader (FLX4) | MISSING | — | P3 |
| Mic / aux input | MISSING | — | P3 |
| Auto gain toggle | ENGINE-ONLY | `ChannelCommand::SetAutoGain`; always on | P3 |

## 6. Effects

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Beat FX (2 units: echo, delay, reverb, flanger, phaser, roll, filter sweep, trans) | ENGINE-ONLY | `od-engine/src/beatfx.rs`, `BeatFxCommand`; 10 FLX10 mapping entries (`beat_fx.*`) all dropped in `handle_action`; no UI panel | **P1** |
| Beat FX: beat select, auto beat, tap, channel assign | ENGINE-ONLY | `SetBeats`, `SetTarget`; same as above | **P1** |
| Larger Beat FX list (ping pong, spiral, helix, pitch, vinyl brake, triplet, shimmer …) | MISSING | 8 kinds today | P3 |
| Sound Colour FX selection (filter, noise, space, crush, echo) | ENGINE-ONLY | `ColorFxKind`, `colorfx.rs`; `color_fx.select` mapped and dropped; no UI | **P1** |
| Release FX | ENGINE-ONLY | Mapped (`beat_fx.release`), no release effect | P2 |
| Pad FX | MISSING | See pad modes | P2 |
| Stem FX (FX on one part) | MISSING | — | P3 |

## 7. Stems / track separation

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Offline separation (vocals, drums, bass, inst), cache, background | DONE | `od-stems` (`worker.rs`, `cache.rs`), `perform.rs::request_stems/poll_stems` | |
| Part mute toggles in UI | DONE | `on_deck_stem` → `SetStemLevel` | |
| Stem levels / part isolator knobs | ENGINE-ONLY | `SetStemLevel` is continuous; controller `StemLevel` dropped; FLX10 part-iso knobs mapped to `ignore` | **P1** (controller coverage) |
| Stem buttons on the controller | ENGINE-ONLY | `DeckAction::StemMuteToggle` exists; FLX10 part buttons mapped to `ignore` (`flx10.toml` "Mix point / part buttons") | **P1** (controller coverage) |
| Separate ahead of time (Prepare, batch) | PARTIAL | `od-cli stems`; UI only separates on load | P2 |

## 8. Sampler

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| 16 slots, one-shot/loop, gain, stop | ENGINE-ONLY | `od-engine/src/sampler.rs`, `SAMPLER_SLOTS = 16`; nothing sends `SamplerCommand::Load` | P2 |
| Load samples / sample library | MISSING | No UI, no store | P2 |
| Banks (bank next/prev) | MISSING | Mapped (`sampler.bank_*`), dropped | P2 |
| Sampler volume, cue | ENGINE-ONLY | `MixerCommand::SetSamplerLevel`; `sampler.volume` mapped and dropped | P2 |
| Sequencer | MISSING | — | P3 |

## 9. Recording

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| One-button mix recording (UI + controller) | DONE | `perform.rs::toggle_recording`, `ui/topbar.slint`, `RecordingToggle` + LED | |
| WAV output | DONE | `od-recorder/src/lib.rs` (hound) | |
| FLAC / MP3 output | MISSING | SPEC §11 | P2 |
| Automatic tracklist (.cue + text) | DONE | `Recorder::mark_track`, `cue_sheet`, `text_tracklist` | |
| Split into per-track files | DONE | `od_recorder::split`, `on_split_recording` | |
| Recordings section in the library | DONE | `recording` table, `refresh_recordings`, rail "RECORDINGS" | |
| Silence auto start/stop | MISSING | SPEC §11 | P2 |
| Output folder / file name setting, recording level | MISSING | Hard-coded `~/Music/OpenDecks Recordings` | P3 |

## 10. Export / USB devices

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Legacy Pioneer export (`export.pdb` + ANLZ DAT/EXT/2EX) | DONE | `od-format-pioneer`, `od-devices/src/pioneer.rs` | |
| Automatic incremental sync (Sync Manager equivalent) | DONE (beyond rekordbox) | `od-devices/src/sync.rs`, `manager.rs`; device sidebar | |
| Device registry, formats, assigned content, rebuild, eject | DONE | `app.rs` `on_register_volume/on_set_device_*/on_rebuild_device/on_eject_device` | |
| Browse / play from a stick | DONE | `on_open_device`, read-only library | |
| OpenDecks native stick format | DONE | `od-format-native` | |
| `exportExt.pdb` (My Tag on players) | MISSING | `pdb/mod.rs` comment: not written | P3 |
| Player settings on stick (`MYSETTING`) | MISSING | SPEC §8.5: defaults or omitted | P3 |
| Device Library Plus / OneLibrary | OUT-OF-SCOPE | SPEC §8.5 / M5 | |
| Pro DJ Link (network export) | OUT-OF-SCOPE | SPEC §10.3 | |

## 11. Preferences and controller settings

| Feature | Status | Evidence / note | Prio |
|---|---|---|---|
| Preferences window | MISSING | Settings only as CLI flags (`apps/opendecks/src/main.rs`); nothing persisted | **P1** |
| Audio device, buffer size, output routing | PARTIAL | Auto-picks a DDJ/FLX device, fixed 128/256 frames (`perform.rs::ensure_engine`); no choice | **P1** |
| Language, key notation, colour-blind, high contrast, motion | PARTIAL | `--lang`, `--keys`, `--colorblind`, `--high-contrast`, `--no-motion` only | **P1** (via preferences) |
| Waveform options (zoom default, phrase display) | MISSING | — | P2 |
| Deck defaults (tempo range, quantize, sync mode, jog mode, auto cue) | MISSING | Engine defaults only | P2 |
| Controller LED feedback | DONE | `perform.rs::feedback` (play, cue, sync, master, key lock, slip, loop, pads, VU) | |
| Soft takeover | DONE | `od-controller/src/takeover.rs` | |
| MIDI learn / MIDI monitor | ENGINE-ONLY | `od-controller/src/learn.rs`, `examples/midi_monitor.rs`; no UI | P3 |
| FLX10 utility settings (jog ring, fader start, USB input …) | ENGINE-ONLY | `od-controller/src/flx10_settings.rs`; no UI (SPEC M4) | P3 |

## 12. Video, lighting, DVS, streaming, cloud

| Feature | Status | Reason |
|---|---|---|
| Video / lyric | OUT-OF-SCOPE | SPEC §10.3 |
| Lighting | OUT-OF-SCOPE | SPEC §10.3 |
| DVS | OUT-OF-SCOPE | SPEC §10.3 |
| Streaming services | OUT-OF-SCOPE | SPEC §10.3 |
| Cloud library sync / Dropbox / mobile app sync | OUT-OF-SCOPE | SPEC §2: offline-first, no accounts |
| Live streaming / mix upload | OUT-OF-SCOPE | SPEC §2 offline-first |
| Edit / remix mode | OUT-OF-SCOPE | Not in SPEC §10 (would need a SPEC change) |

---

## Counts

| Status | Rows |
|---|---|
| DONE | 65 |
| PARTIAL | 18 |
| ENGINE-ONLY | 25 |
| MISSING | 40 |
| OUT-OF-SCOPE | 9 |
| **Total** | **157** |

## P1 gaps and implementation notes

1. **Persist cue edits made in Perform.** Hot cues (incl. loop hot cues) and
   memory cues set or deleted on pads/controller only change engine state.
   *`od-ui` (`perform.rs`)*: after `HotCue`/`DeleteHotCue`/memory-cue
   actions, read the deck's cue table from `DeckState` (or mirror the
   command), build `Vec<Cue>` and call `Library::set_cues` with a journal
   label, unless the deck plays from a stick (`loaded_from_stick`). Wire the
   dropped `MemoryCueSet/MemoryCueDelete` actions the same way. Sync then
   follows automatically via the change log.
2. **Beat FX and Colour FX selection.** Engine is complete; nothing drives it.
   *`od-ui` (`perform.rs::handle_action`)*: translate `BeatFxAction`
   (toggle/on, level, beat up/down, auto beat, select next/prev, assign,
   tap) into `Command::BeatFx`, and `ColorFxAction::Select/Next` into
   `ChannelCommand::SetColorFx` for all channels. *`ui/perform.slint`*: a
   compact FX strip (type, beats, depth, target, on) per unit next to the
   mixer; add FX LEDs in `feedback`. Release FX can come later.
3. **Track metadata editing.** Rating, colour, comment and core tags are
   stored but read-only. *`od-ui`*: inline rating click in `table.slint`,
   colour/comment columns, a track-info panel in Prepare that calls
   `Library::set_tags` (already journalled, already marks sticks out of
   date). Add colour and comment as optional columns (see column choice, P2).
4. **Playlist and set ordering, drag and drop.** *`od-ui`*: Slint drag
   support (or move up/down + cut/paste rows as a first step) calling
   `nodes.rs::move_entries`, `add_to_node`, `move_node`; file drop from the
   OS into the library uses the existing import path. Sets depend on order,
   so this is core to SPEC §5.
5. **Complete the Prepare cue editor.** *`od-ui` (`app.rs`, `ui/player.slint`)*:
   delete/move memory cues, store the current loop as a memory or hot-cue
   loop (`Cue.loop_end`), edit label and colour (palette from
   `od-format-pioneer/src/palette.rs` so players show it), and draw cue
   colours (add a colour field to `CueMark`).
6. **Grid editor: tap tempo and typed BPM; Tap BPM on the controller.**
   *`od-ui` (`app.rs::edit_grid`)*: tap button averaging intervals, numeric
   BPM entry building a constant grid from the current downbeat
   (`Beatgrid::constant`). Wire `DeckAction::TapBpm` to the same helper for
   the loaded track.
7. **Controller coverage for mapped but dropped actions.** `handle_action`
   falls through to `_ => None` for many FLX10/FLX4 inputs that the mappings
   already decode. *`od-engine`*: add `DeckCommand::Search { forward, on }`
   (fast scan) and a `KeySync` (shift to master key via `SetKeyShift`).
   *`od-ui`*: wire `Search`, `KeySync`, `StemLevel`/`StemMuteToggle`
   (and map the FLX10 part buttons and part-iso knobs in `flx10.toml`
   instead of `ignore`), `Sampler::Volume`, `Browse::Related/ViewToggle`.
   Each needs a line in `docs/controller-mappings.md` and a test in
   `crates/od-controller/tests/mappings.rs`.
8. **Preferences window with persisted settings.** *`od-ui`* (+ a small
   settings store, e.g. a `meta` row or a TOML file in the data dir):
   audio device, buffer size and headphone routing (`od-audio-io::list_devices`,
   restart the host on change while not playing), language, key notation,
   colour-blind / high contrast / motion, and the engine options that exist
   but have no switch (EQ mode, fader and crossfader curves, default tempo
   range). CLI flags stay as overrides.

### P2 summary

Column choice and filter bar; My Tag; related-tracks / suggestion panel
(`suggest_next`); own play history with a history view; tree node reorder;
set backups/start time UI; full Prepare keyboard shortcuts and controller
tree navigation; phrase display (in progress); key shift + key-shift pads +
key sync; pad FX and release FX; sampler loading, banks and volume; pad LEDs
for all modes; waveform zoom; EQ/fader/crossfader curve settings (if not
covered by P1-8); master cue and booth output; FLAC/MP3 recording and silence
auto start; batch stem separation in Prepare.

### P3 summary

Artwork display, duplicate check, XML/iTunes exchange, consolidate UI,
variable-tempo grid editing, BPM range setting, waveform style options,
vocal position, hot cue banks, active loop, beat-jump size setting, keyboard
and slicer pad modes, larger Beat FX list, stem FX, sampler sequencer, Smart
CFX/Smart Fader, mic/aux input, auto-gain toggle, recording folder/level
settings, `exportExt.pdb`, `MYSETTING`, MIDI learn UI, FLX10 utility settings
UI and jog displays (M4).
