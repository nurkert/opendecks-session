<!-- Generated from tests/parity/features.toml by `cargo run -p od-parity --bin parity -- matrix`. Do not edit by hand. -->

# Feature matrix (OpenDecks Session)

Snapshot: 2026-11-06, from a read of the source tree.

**Summary:** 213 features; 206 in scope: 83 DONE (40 %), 30 PARTIAL, 27 ENGINE-ONLY, 66 MISSING; 2 deferred to a later milestone; 5 out of scope (SPEC §1).

The list covers the professional feature set a DJ expects when preparing music and performing
with a controller such as the DDJ-FLX10 / DDJ-FLX4 (SPEC §1, §5.5, §10, §11, §12). Behaviour,
layout and naming in OpenDecks follow [SPEC.md](SPEC.md) and [DESIGN.md](../DESIGN.md).
Statuses come from the 2026-11-06 read of the source tree; this file is generated from
`tests/parity/features.toml`, see [parity.md](parity.md).

Evidence written before the `od-ui` rebuild names `perform.rs` (`crates/od-ui/src/perform.rs`); that
code now lives in `decks.rs`, `controller.rs`, `mixer.rs` and `recorder.rs`. Other short paths:
`app.rs` = `crates/od-ui/src/app.rs`, `command.rs` = `crates/od-engine/src/command.rs`,
`action.rs` = `crates/od-controller/src/action.rs`, `flx10.toml` = `mappings/pioneer/ddj-flx10.toml`.

## Legend

| Status | Meaning |
|---|---|
| DONE | Works end to end, including UI and/or controller |
| ENGINE-ONLY | Backend (engine, library, format) exists; no UI and no controller path |
| PARTIAL | Usable, but an expected part is missing (noted) |
| MISSING | Nothing in the code |
| OUT-OF-SCOPE | Excluded by SPEC §1 (streaming, cloud, video, lighting, DVS) |

A milestone after the status ("deferred to M6") marks an in-scope feature that SPEC §15 schedules later; it is tracked but not counted as a gap. The Tests column counts the automated tests linked in `features.toml` (SPEC §16.1).

Priority (gaps only): **P1** = an experienced DJ hits it in the first session and the app feels broken or foreign without it; **P2** = expected, regular use; **P3** = niche, power-user or later.

---

## 1. Browser / collection

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Collection table with sortable columns | DONE | `ui/table.slint` header cells; `app.rs::sort_key` (title, artist, genre, BPM, key, time, rating, date added) | 2 |  |
| Column choice (album, label, comment, colour, date added, play count, bitrate, My Tag, …) | MISSING | Fixed column set in `table.slint`; `TrackRow` has `album` but no header for it | — | P2 |
| Artwork in browser / deck | PARTIAL | Stored (`schema.sql` `artwork`, `tracks.rs::artwork`) and exported to sticks (`pdb/rows.rs`); not shown in the UI | — | P3 |
| Tree: folders, playlists, smart lists, sets | DONE | `nodes.rs::create_node/rename_node/delete_node`; `app.rs` `on_create_node`, `on_rename_node`, `on_delete_node` | 2 |  |
| Reorder tree nodes (drag) | ENGINE-ONLY | `nodes.rs::move_node`; no UI | 1 | P2 |
| Add tracks to playlist | DONE | `app.rs` `on_add_to_node` | 1 |  |
| Reorder playlist / set entries | ENGINE-ONLY | `nodes.rs::move_entries`; not called from `od-ui` | — | **P1** |
| Drag and drop (table → tree, Finder → library) | MISSING | No drag/drop handlers in `ui/*.slint` (with reorder) | — | **P1** |
| Search | DONE | `tracks.rs::search`; `app.rs` `on_search` | 2 |  |
| Filter (BPM range, key, rating, My Tag, relative to master deck) | PARTIAL | Smart-list rules can express it (`smart.rs` `Field`/`Op`, incl. `HarmonicWith`); no quick filter bar in the browser | 1 | P2 |
| Rating (stars) | PARTIAL | Shown and sortable (`TrackRow.rating`); not editable (`tracks.rs::set_tags` is unused by the UI) | — | **P1** |
| Track colour | ENGINE-ONLY | `TrackTags.color` (`od-core/src/track.rs`), imported and exported; no column, no editor | — | **P1** |
| Comments | ENGINE-ONLY | `TrackTags.comment`; smart-rule field `Comment`; no column, no editor | — | **P1** |
| Edit tags (title, artist, album, genre, label, year …) | ENGINE-ONLY | `tracks.rs::set_tags` (journalled); no track-info editor | 1 | **P1** |
| My Tag | ENGINE-ONLY | Model and undoable edits in `od-library` (`mytags.rs`), searchable and editable through the MCP server; no browser column or editor yet; importer skips My Tag smart conditions (`od-import-rekordbox/src/tests.rs`) | 1 | P2 |
| Tag list / palette (holding area) | MISSING | — | — | P3 |
| Related tracks | ENGINE-ONLY | `sets.rs::suggest_next`; FLX10 `browse.related` is mapped but dropped in `handle_action` | 1 | P2 |
| Track suggestion / harmonic matching | PARTIAL | Key-distance column vs. selected track (`app.rs` `key_distance`), set flow fit (`sets.rs::transition`); no suggestion list | 2 | P2 |
| Intelligent (smart) playlists | DONE | `od-library/src/smart.rs`, text rule editor `od-ui/src/smart_text.rs`, `on_set_smart_rule` | 4 |  |
| Histories: player histories from sticks | DONE | `od-devices/src/pioneer.rs` history proposals → `app.rs::resolve_proposal` creates a "History" folder | — |  |
| Histories: own play history | PARTIAL | `tracks.rs::record_play` is only called while recording (`perform.rs::frame`); no history view | — | P2 |
| Track preview (Prepare) | DONE | `perform.rs::toggle_preview` on a preview deck; Space key | — |  |
| Loaded-on-deck indicator | DONE | `TrackRow.deck` | — |  |
| Duplicate track check | MISSING | — | — | P3 |
| Import folder / files | DONE | `app.rs` `on_import_folder`, `od-cli import` | — |  |
| rekordbox library + stick migration | DONE | `od-import-rekordbox`, `od-ui/src/import.rs` | 7 |  |
| rekordbox preferences and keyboard shortcuts migration | ENGINE-ONLY | `crates/od-import-rekordbox/src/settings.rs`, `apps/od-cli/src/main.rs::rekordbox_settings_cmd`; Reader + mapping report (`od-cli rekordbox-settings`); applying to the preferences and an import-flow button wait for the persisted preferences; (SPEC §7) | 7 | P2 |
| rekordbox XML / iTunes library import-export (compatibility) | MISSING | — | — | P3 |
| Missing files / relocate | DONE | `relocate.rs::check_missing/find_relocations`; `on_find_moved_files` | 1 |  |
| Consolidate | ENGINE-ONLY | `relocate.rs::consolidate`; no UI | 1 | P3 |
| Undo/redo with history | DONE (beyond the usual) | `journal.rs`; Cmd-Z / Shift-Cmd-Z in `app.rs::key` | 3 |  |
| Sets with transition notes and flow view | DONE (own) | `sets.rs::set_flow`, `on_set_transition_note`, `ui/flow.slint` | 3 |  |
| Set backup tracks / planned start | ENGINE-ONLY | `nodes.rs::set_entry_backups/set_set_meta` | — | P2 |
| Keyboard shortcuts for Prepare | DONE | `keymap.rs::registry`, `commands.rs::run`, `docs/keyboard.md`; One command registry (177 actions, conventional DJ-software defaults for decks 1/2); Preferences > Keyboard rebinds with conflict detection, reset, TOML import/export | 4 | P2 |
| Browse from controller | PARTIAL | Rotate / press / back handled (`handle_action` `BrowseAction`); tree navigation, view toggle, shift-sort and related dropped | 1 | P2 |
| Explorer: browse folders on disk and play directly | DONE | `crates/od-ui/src/explorer.rs::ExplorerState::rows`, `crates/od-ui/src/explorer.rs::open_folder`, `crates/od-ui/ui/sidebar.slint::ExplorerItem`; User-added roots (persisted in library meta) as a nested folder tree of any depth: sub-folders are listed lazily on expand/select, each probed one level so only expandable folders get a mark; hidden and player data folders are skipped. Selecting a folder expands it and lists its audio files; (SPEC §5.5; UX §2.2) | 5 |  |
| Explorer: "Include subfolders" lists the files of a folder and all its sub-folders | DONE (beyond rekordbox) | `crates/od-ui/src/explorer.rs::list_files`, `crates/od-ui/src/explorer.rs::set_recursive`, `crates/od-ui/ui/app.slint`; Toggle in the folder list header (persisted as `explorer.recursive`); rekordbox lists only the folder itself. Depth-first, each folder by name, symlink loops followed once, capped at 32 levels / 20 000 files; (SPEC §5.5) | 3 |  |
| Explorer: loading a file adds it to the collection and analyses it in the background | PARTIAL | `crates/od-ui/src/explorer.rs::start_import`, `crates/od-ui/src/explorer.rs::poll`; Implemented (`explorer.rs` `start_import` adds the file in the background via `workers::import_file`, `poll` wakes analysis and loads it); no automated test yet; (SPEC §5.5) | — | **P1** |
| Source rail switching the tree (Collection, Playlists, Related, Explorer, Devices, Histories, Recordings) | PARTIAL | `crates/od-ui/ui/rail.slint`; Rail with sections exists; Explorer, Histories, Related and Tag list sources missing; (UX §1.6, §4 P4.9) | — | P2 |
| Re-orderable columns and multi-sort | MISSING | Fixed column order, one sort key (`browser/tracks.rs`); (SPEC §5.5) | — | P3 |
| Load a track: drag to deck, double-click (Prepare), context menu Load ▸ Deck n | PARTIAL | `crates/od-ui/src/decks.rs::free_deck`; Activate/double-click and controller load work; drag to deck and the context menu do not exist; (UX §2.1) | 1 | **P1** |
| Drag tracks from the browser (and Explorer, Finder) onto a deck or the Prepare player | PARTIAL | Done for list and Explorer rows onto decks and the Prepare player (drag ghost, deck highlight; verified by driving real mouse events). Missing: drop from Finder, automated UI test. Owner request 2026-11-06. Drop target highlights while dragging (deck colour); dropping on a playing deck asks no dialog but needs a deliberate drop on the deck area (loads unless the deck is playing and load lock is on); Finder/Explorer files are imported on drop.; (SPEC §5.5) | — | **P1** |
| Track row context menu (Load ▸, Add to playlist ▸, Analyse, Show in Finder, …) | MISSING | No popup menus in `crates/od-ui/ui`; (UX §1.9) | — | P2 |
| Info panel (track details beside the list) | MISSING | (SPEC §5.5; UX §1.6) | — | P2 |
| Sub-browser (second tree + list for dragging between playlists) | MISSING | (UX §1.6, §4 P4.12) | — | P3 |
| Automatic database backups (rotating) | DONE (beyond the usual) | `crates/od-library/src/backup.rs`; (SPEC §5.3) | 1 |  |
| Export / import the whole library as one archive (database + analysis, optional audio) | MISSING | (SPEC §5.3) | — | P3 |
| MCP server: full library access for preparing sets with an LLM | DONE | `apps/od-mcp/src/tools.rs`, `apps/od-mcp/src/server.rs`; Owner request 2026-11-06 (SPEC §5.6). `opendecks-mcp` (MCP 2025-06-18, stdio or loopback HTTP): 8 read and 10 write tools, track/node resources, build_set and fix_transitions prompts; each write is one undo step "MCP: …" plus change-log rows (stick sync); setup in docs/mcp.md; (SPEC §5.6) | 8 | **P1** |

## 2. Analysis

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| BPM + beatgrid incl. downbeats | DONE | `od-analysis/src/tempo.rs::detect_beatgrid`; background worker `od-ui/src/workers.rs` | 6 |  |
| Variable-tempo grid | PARTIAL | Model supports it (`od-core/src/beatgrid.rs`); editor only rebuilds constant grids (`on_grid_scale`) | 1 | P3 |
| Grid editor: shift, ×2/÷2, set downbeat, lock | DONE | `app.rs` `on_grid_shift/on_grid_scale/on_grid_downbeat/on_grid_lock` | 3 |  |
| Grid editor: tap tempo, type BPM | MISSING | Required by SPEC §6 | — | **P1** |
| BPM analysis range setting | MISSING | — | — | P3 |
| Key detection, notation Camelot/Open Key/musical | DONE | `od-analysis/src/key.rs`; `--keys` flag, `KeyNotation` | 7 |  |
| Phrase analysis | DONE | Analysed (`od-analysis/src/phrase.rs`), stored and exported (`PSSI` in `anlz/build.rs`). Display: see `deck-phrase-display` | 2 |  |
| Waveform: own 3-band colour | DONE | `od-analysis/src/waveform.rs`, `render.rs` (+ colour-blind palette) | 4 |  |
| Waveform style options (RGB / blue / 3-band) | MISSING | One style by design; stick export still writes `PWAV/PWV2–PWV7` for players | — | P3 |
| Vocal position | MISSING | Could be derived from the vocal stem | — | P3 |
| Loudness + auto gain | DONE | `od-analysis/src/loudness.rs`; gain in `perform.rs::load_track`, engine default on (`mixer.rs` `auto_gain: true`); no toggle in UI | 3 |  |
| Re-analyse selected, never overwrite manual/imported | DONE | `on_analyse_selected`; `DataSource` priority in `tracks.rs::store_analysis` | 3 |  |
| Phrase editing (GRID EDIT \| PHRASE EDIT, cut / clear) | MISSING | (UX §2.5) | — | P3 |

## 3. Cues and loops

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 8 hot cues A–H (Prepare) | DONE | `app.rs` `on_hot_cue/on_remove_hot_cue`; keys 1–8 | 2 |  |
| 8 hot cues A–H (Perform, pads/UI) | DONE | `DeckCommand::HotCue`, `od-ui/src/decks.rs::persist_cue_events`; Set/delete on pads or the controller are saved to the library (one undo step each) and reach sticks through the normal sync | 2 | **P1** |
| Hot cue banks / hot cue bank lists | MISSING | — | — | P3 |
| Memory cues: add (Prepare) | DONE | `on_add_memory_cue` | — |  |
| Memory cues: delete, move (Prepare) | MISSING | No callback | — | **P1** |
| Memory cues: call prev/next (Perform) | DONE | `MemoryCueJump` via controller | — |  |
| Memory cue set/delete from controller | ENGINE-ONLY | Mapped (`deck.memory_cue_set/delete` in `flx10.toml`), dropped in `handle_action` | — | **P1** |
| Saved loops (memory/hot-cue loops) | PARTIAL | Model, import, export and playback exist (`Cue.loop_end`); cannot be created in Prepare; loop hot cues made in Perform are not persisted | 1 | **P1** |
| Active loop (auto-engaged saved loop) | MISSING | — | — | P3 |
| Cue colours | PARTIAL | `Cue.color` imported/exported; not drawn (`CueMark` has no colour), not editable | — | **P1** |
| Cue comments / labels | PARTIAL | Shown on pads (`HotCueSlot.label`); not editable | — | **P1** |
| Auto cue on load (first memory cue / first beat) | DONE | `DeckCommand::Load` behaviour (`command.rs` doc, `deck.rs`) | — |  |
| Cues set on players → library | DONE (beyond the usual) | `od-devices/src/pioneer.rs` proposals, `on_accept_proposal` | 2 |  |

## 4. Deck / player

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 2 / 4 decks, deck layers | DONE | `on_set_deck_count`, FLX10 auto 4 decks (`perform.rs::frame`), `DeckSelect` in `od-controller/src/session.rs` | 3 |  |
| Play / Cue (CDJ cue logic) | DONE | `DeckCommand::Cue`; UI + controller | 2 |  |
| Jog: vinyl scratch, bend, search; vinyl mode toggle | DONE | `DeckCommand::JogTurn/JogTouch/SetJogMode`, `handle_action` | 2 |  |
| Jog display (FLX10 screens) | MISSING, M4 | Needs the non-MIDI protocol (SPEC M4, `docs/ddj-flx10-usb-protocol.md`) (M4) | — | P3 |
| Tempo fader, ranges ±6/10/16/WIDE, reset | DONE | `TempoRange`; controller cycles range; UI shows it but has no range button | 3 |  |
| Master tempo / key lock (Rubber Band) | DONE | `od-engine/src/keylock`, UI toggle + controller | 4 |  |
| Key shift (±semitones) | ENGINE-ONLY | `DeckCommand::SetKeyShift`; only `KeyReset` reaches it | — | P2 |
| Key sync | MISSING | Mapped (`deck.key_sync`); no engine command | — | P2 |
| Beat sync + master deck | DONE | `SetSync`, `SetMasterDeck`; UI + controller | 1 |  |
| Quantize | DONE | `Command::SetQuantize`; UI + controller | 1 |  |
| Slip | DONE | `SetSlip`; UI + controller | 1 |  |
| Reverse | DONE | `SetReverse` via controller; no UI button | 2 |  |
| Censor (slip reverse) | DONE | `DeckAction::Censor` in `handle_action` | — |  |
| Beat jump | DONE | `DeckCommand::BeatJump`; UI ±4, controller, pad mode | 1 |  |
| Beat jump size setting | MISSING | Fixed ±4 in UI, pad sizes fixed in `handle_action` | — | P3 |
| Manual loop in/out, reloop/exit, ½ / 2× | DONE | `LoopIn/LoopOut/Reloop/LoopExit/LoopHalve/LoopDouble` | 1 |  |
| Auto beat loop (1/32 … 32) | DONE | `DeckCommand::AutoLoop`; UI size stepper | 1 |  |
| Loop roll | DONE | `DeckCommand::LoopRoll` via controller | 1 |  |
| Needle search | DONE | `NeedleSearch`; overview click + FLX touch strip | 1 |  |
| Search (FF/REW buttons) | MISSING | Mapped (`deck.search`); no engine command, dropped (controller coverage) | — | **P1** |
| Tap BPM | MISSING | Mapped (`deck.tap_bpm`), dropped (with grid tap) | — | **P1** |
| Pad mode: Hot Cue | DONE | `PadMode::HotCue` in `handle_action` | 1 |  |
| Pad mode: Beat Loop | DONE | `PadMode::BeatLoop` → `AutoLoop` | 2 |  |
| Pad mode: Beat Jump | DONE | `PadMode::BeatJump` | 2 |  |
| Pad mode: Sampler | PARTIAL | Triggers slots, but no way to load samples (see §8) | — | P2 |
| Pad mode: Pad FX 1/2 | MISSING | `PadMode::PadFx1/2` exist in `action.rs`; no effect logic | — | P2 |
| Pad mode: Keyboard | MISSING | `PadMode::Keyboard` selectable; no logic (needs key shift) | — | P3 |
| Pad mode: Key Shift | ENGINE-ONLY | `PadMode::KeyShift` selectable; engine `SetKeyShift` exists; pads dropped | — | P2 |
| Pad mode: Slicer | MISSING | Not in `PadMode` | — | P3 |
| Pad LED feedback per mode | PARTIAL | Only hot-cue pad LEDs (`perform.rs::feedback`) | 1 | P2 |
| Waveform zoom in Perform | MISSING | Fixed `DETAIL_SPAN` 8 s (`perform.rs`) | — | P2 |
| Waveform overview + scrolling detail, cue markers, beat ticks | DONE | `render.rs::overview/detail`, `ui/plate.slint` | 3 |  |
| Phrases on the overview and enlarged waveform, with display options | PARTIAL | `crates/od-ui/src/player.rs::phrase_marks`, `crates/od-ui/src/render.rs`; Phrase strip is painted; no opt-in toggles for enlarged / overview waveform and no phrase types; (SPEC §10.1; UX §2.5) | 1 | P2 |
| Deck header: title, artist, artwork, BPM, remaining / elapsed time | PARTIAL | `crates/od-ui/src/format.rs`; Artwork not shown (see `browser-artwork`); (SPEC §10.1; UX §1.3) | 3 | P2 |
| Jog / platter readout in the UI: position, remaining, phrase and bar counter | PARTIAL | Bar counter and phrase readout not verified in the rebuilt UI; (SPEC §10.1; UX §1.3) | — | P2 |
| Full-width enlarged waveform zone, one stacked lane per deck (2 and 4 decks), centre playhead | PARTIAL | `crates/od-ui/src/decks.rs`; Lanes per deck; zone layout and zoom column per UX §1.3 not complete; (SPEC §10.1; UX §1.3, §4 P1.1) | — | **P1** |
| Pad mode: Stems (part mutes and stem FX on pads) | MISSING | (UX §2.4) | — | P2 |
| Pad mode: Memory Cue | MISSING | (UX §1.3) | — | P3 |
| Pad mode: Sequence Call | MISSING | (UX §1.3) | — | P3 |
| Pad mode: Active Censor | MISSING | (UX §1.3) | — | P3 |
| CDJ practice mode: CDJ-3000-like screens and behaviour driven by the controller | MISSING | Owner request 2026-11-06 (SPEC §10.3c).; (SPEC §10.3c) | — | P2 |

## 5. Mixer

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Trim | DONE | `ChannelCommand::SetTrim`; UI knob + controller | 2 |  |
| 3-band EQ (isolator) | DONE | `dsp/eq.rs` default `Isolator`; UI + controller | 3 |  |
| EQ mode classic/isolator | ENGINE-ONLY | `ChannelCommand::SetEqMode`; no setting | — | P2 |
| Colour FX knob (filter) | DONE | `SetColorFxAmount`; UI + controller (default kind = filter) | 1 |  |
| Channel fader | DONE | UI + controller | 3 |  |
| Channel fader curve | ENGINE-ONLY | `FaderCurve` / `SetFaderCurve`; no setting | 1 | P2 |
| Crossfader | DONE | UI + controller | 2 |  |
| Crossfader curve | ENGINE-ONLY | `CrossfaderCurve`; no setting | 1 | P2 |
| Crossfader assign A/THRU/B | DONE | Controller (`MixerAction::CrossfaderAssign`); no UI control | 1 |  |
| Headphone cue, mix, level | DONE | `SetCue`, `SetHeadphoneMix/Level`; UI + controller | 2 |  |
| Master level, VU meters, limiter | DONE | `SetMasterLevel`, meters in `ChannelView`, limiter on by default | 3 |  |
| Master cue | MISSING | Mapped (`master_cue`), no engine function | — | P2 |
| Booth level / booth output | MISSING | Mapped (`booth_level`), no engine bus | — | P2 |
| Smart CFX / Smart Fader (FLX4) | MISSING | — | — | P3 |
| Mic / aux input | MISSING | — | — | P3 |
| Auto gain toggle | ENGINE-ONLY | `ChannelCommand::SetAutoGain`; always on | — | P3 |
| STEM button per mixer channel | MISSING | (UX §1.3, §2.4) | — | P2 |

## 6. Effects

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Beat FX (2 units: echo, delay, reverb, flanger, phaser, roll, filter sweep, trans) | ENGINE-ONLY | `od-engine/src/beatfx.rs`, `BeatFxCommand`; 10 FLX10 mapping entries (`beat_fx.*`) all dropped in `handle_action`; no UI panel | 2 | **P1** |
| Beat FX: beat select, auto beat, tap, channel assign | ENGINE-ONLY | `SetBeats`, `SetTarget`; same as above | 1 | **P1** |
| Larger Beat FX list (ping pong, spiral, helix, pitch, vinyl brake, triplet, shimmer …) | MISSING | 8 kinds today | — | P3 |
| Sound Colour FX selection (filter, noise, space, crush, echo) | ENGINE-ONLY | `ColorFxKind`, `colorfx.rs`; `color_fx.select` mapped and dropped; no UI | — | **P1** |
| Release FX | ENGINE-ONLY | Mapped (`beat_fx.release`), no release effect | — | P2 |
| Pad FX | MISSING | See pad modes | — | P2 |
| Stem FX (FX on one part) | MISSING | — | — | P3 |
| FX panel (FX1 \| FX2 with three slots, beat value, depth, assign, AUTO, TAP; CFX row) | MISSING | `crates/od-ui/src/fx.rs`; Module exists, panel not built; (SPEC §10.2; UX §1.5) | — | **P1** |
| Colour FX: pitch, dub echo, sweep | MISSING | (SPEC §10.2) | — | P3 |

## 7. Stems / track separation

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Offline separation (vocals, drums, bass, inst), cache, background | DONE | `od-stems` (`worker.rs`, `cache.rs`), `perform.rs::request_stems/poll_stems` | 5 |  |
| Part mute toggles in UI | DONE | `on_deck_stem` → `SetStemLevel` | 2 |  |
| Stem levels / part isolator knobs | ENGINE-ONLY | `SetStemLevel` is continuous; controller `StemLevel` dropped; FLX10 part-iso knobs mapped to `ignore` (controller coverage) | 2 | **P1** |
| Stem buttons on the controller | ENGINE-ONLY | `DeckAction::StemMuteToggle` exists; FLX10 part buttons mapped to `ignore` (`flx10.toml` "Mix point / part buttons") (controller coverage) | — | **P1** |
| Separate ahead of time (Prepare, batch) | PARTIAL | `od-cli stems`; UI only separates on load | — | P2 |
| Separation model downloaded once on demand, nothing leaves the machine | DONE | `crates/od-stems/src/model.rs`; (SPEC §10.3) | 1 |  |
| Deck shows separation progress; stem controls visible (dimmed) while separating | PARTIAL | Separation state is tracked; controls are not shown dimmed per UX §4 P5.14; (SPEC §10.3; UX §2.4) | — | P2 |
| Raspberry Pi plays parts separated on the desktop | PARTIAL | Cached parts attach on load; carrying the cache onto sticks not verified; (SPEC §10.3) | 1 | P2 |

## 8. Sampler

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 16 slots, one-shot/loop, gain, stop | ENGINE-ONLY | `od-engine/src/sampler.rs`, `SAMPLER_SLOTS = 16`; nothing sends `SamplerCommand::Load` | 1 | P2 |
| Load samples / sample library | MISSING | No UI, no store | — | P2 |
| Banks (bank next/prev) | MISSING | Mapped (`sampler.bank_*`), dropped | — | P2 |
| Sampler volume, cue | ENGINE-ONLY | `MixerCommand::SetSamplerLevel`; `sampler.volume` mapped and dropped | — | P2 |
| Sequencer | MISSING | — | — | P3 |
| Sampler panel (2 x 8 slots, gain, cue, BPM, bank row) | MISSING | `crates/od-ui/src/sampler.rs`; Module exists, panel not built; (SPEC §10.3a; UX §1.5) | — | P2 |
| Gate mode, quantised triggering, sample sync to the master tempo | MISSING | (SPEC §10.3a) | — | P2 |

## 9. Recording

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| One-button mix recording (UI + controller) | DONE | `perform.rs::toggle_recording`, `ui/topbar.slint`, `RecordingToggle` + LED | 2 |  |
| WAV output | DONE | `od-recorder/src/lib.rs` (hound) | 1 |  |
| FLAC / MP3 output | MISSING | SPEC §11 | — | P2 |
| Automatic tracklist (.cue + text) | DONE | `Recorder::mark_track`, `cue_sheet`, `text_tracklist` | 2 |  |
| Split into per-track files | DONE | `od_recorder::split`, `on_split_recording` | 1 |  |
| Recordings section in the library | DONE | `recording` table, `refresh_recordings`, rail "RECORDINGS" | 1 |  |
| Silence auto start/stop | MISSING | SPEC §11 | — | P2 |
| Output folder / file name setting, recording level | MISSING | Hard-coded `~/Music/OpenDecks Recordings` | — | P3 |
| Recording panel: source, level, meter, elapsed time, file size, live tracklist | PARTIAL | `crates/od-ui/src/recorder.rs::frame`; Elapsed time and REC exist; level, meter, size and live tracklist missing; (SPEC §11; UX §2.3) | — | P2 |

## 10. Export / USB devices

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Legacy Pioneer export (`export.pdb` + ANLZ DAT/EXT/2EX) | DONE | `od-format-pioneer`, `od-devices/src/pioneer.rs` | 9 |  |
| Automatic incremental sync (Sync Manager equivalent) | DONE (beyond the usual) | `od-devices/src/sync.rs`, `manager.rs`; device sidebar | 4 |  |
| Device registry, formats, assigned content, rebuild, eject | DONE | `app.rs` `on_register_volume/on_set_device_*/on_rebuild_device/on_eject_device` | 4 |  |
| Browse / play from a stick | DONE | `on_open_device`, read-only library | 1 |  |
| OpenDecks native stick format | DONE | `od-format-native` | 3 |  |
| `exportExt.pdb` (My Tag on players) | MISSING | `pdb/mod.rs` comment: not written | — | P3 |
| Player settings on stick (`MYSETTING`) | MISSING | SPEC §8.5: defaults or omitted | — | P3 |
| Device Library Plus / OneLibrary | MISSING, deferred to M6 | SPEC §8.5 / §15 M6: OneLibrary (`exportLibrary.db`) export comes later; tracked, not counted | — | P3 |
| Pro DJ Link (network export) | MISSING, deferred to M6 | SPEC §10.3b "Pro DJ Link (later)", §15 M6; tracked, not counted | — | P3 |

## 11. Preferences, audio and controllers

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Preferences window | DONE | `ui/preferences.slint`, `preferences.rs::open`, `settings.rs::Settings`; Separate window (⌘, / Ctrl+, from the native menu and the keymap, gear in the top bar, Esc / ⌘W closes): Audio, View, Analysis, Controller, Keyboard, Recording, Advanced; stored in the library meta, CLI flags override for one run | 4 | **P1** |
| Audio device, buffer size, output routing | DONE | `engine.rs::audio_config`, `preferences.rs::refresh`; Preferences > Audio: output device (automatic = controller interface), buffer 64–1024, sample rate, headphones auto / 3-4 / off; applies when audio next starts | 2 | **P1** |
| Language, key notation, colour-blind, high contrast, motion | DONE | `preferences.rs::apply_live`, `shell.rs::apply_theme`; Preferences > View, applied live in both windows; UI scale (75–200 %) after restart | 3 | **P1** |
| Waveform options (zoom default, phrase display) | PARTIAL | `settings.rs::ViewSettings`; Phrase display on/off (live, Prepare and Perform); waveform colour 3-band/blue is stored but the renderer does not read it yet; no zoom default | — | P2 |
| Deck defaults (tempo range, quantize, sync mode, jog mode, auto cue) | MISSING | Engine defaults only | — | P2 |
| Controller LED feedback | DONE | `perform.rs::feedback` (play, cue, sync, master, key lock, slip, loop, pads, VU) | 3 |  |
| Soft takeover | DONE | `od-controller/src/takeover.rs` | 4 |  |
| MIDI learn / MIDI monitor | ENGINE-ONLY | `od-controller/src/learn.rs`, `examples/midi_monitor.rs`; no UI | 4 | P3 |
| FLX10 utility settings (jog ring, fader start, USB input …) | ENGINE-ONLY | `od-controller/src/flx10_settings.rs`; no UI (SPEC M4) | 3 | P3 |
| Audio output on macOS (CoreAudio) | DONE | `crates/od-audio-io/src/output.rs`; (SPEC §10.4) | 1 |  |
| Linux audio: PipeWire / JACK, ALSA direct on the Pi | PARTIAL | `crates/od-audio-io/src/devices.rs`; Host selection exists; no automated test per backend; (SPEC §10.4) | — | P2 |
| DDJ-FLX10 mapping (4 decks, LEDs) | DONE | `mappings/pioneer/ddj-flx10.toml`; (SPEC §9.1) | 3 |  |
| DDJ-FLX4 mapping (2 decks, LEDs) | DONE | (SPEC §9.1) | 2 |  |
| Browser font size and line spacing | MISSING | (UX §1.8, §4 P5.17) | — | P2 |

## 12. Shell, layouts and panels

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Edit mode (track editing / remix) | MISSING | Track edit / remix mode. SPEC §1 does not exclude it, so it counts as in scope: implement it or amend SPEC §1 with a reason | — | P3 |
| Prepare / Perform mode switch that keeps loaded tracks | DONE | `crates/od-ui/src/shell.rs`; (SPEC §3.1) | — |  |
| Perform layouts: 2 / 4 decks horizontal and vertical, Browse | PARTIAL | 2 and 4 decks; vertical layouts and Browse missing; (SPEC §10.1; UX §1.4) | — | P2 |
| Prepare layouts: 1 player, 2 players, dual, simple, full browser | MISSING | (UX §1.7) | — | P2 |
| Top-bar panel toggles: FX, Mix Point Link, Sampler, Mixer, Recording | MISSING | (UX §1.2, §2.8) | — | P2 |
| Mix Point Link (mix in / mix out points between decks) | MISSING | (UX §1.5; SPEC §16.3) | — | P3 |
| Draggable splitters with persisted sizes, double-click reset | PARTIAL | `crates/od-ui/src/shell.rs`; Sidebar width and upper height persist; per-layout sizes and waveform-zone collapse / expand missing; (SPEC §12; UX §2.6) | 1 | P2 |
| Status band: analysis / sync state, list summary (tracks, time, size) | PARTIAL | `crates/od-ui/src/shell.rs`; List summary per UX §1.6 not verified; (SPEC §12; UX §1.6) | 1 | P3 |
| UI scaling 0.75–2.0 without clipping | MISSING | (SPEC §12, §16.4) | — | P2 |
| Colour-blind palette and high-contrast theme | PARTIAL | CLI flags only (`--colorblind`, `--high-contrast`); see `prefs-display`; (SPEC §12) | — | P2 |
| English and German UI | DONE | `crates/od-i18n`; (SPEC §2) | 2 |  |
| Lite / Full performance profiles | DONE | (SPEC §3.2) | 2 |  |
| Raspberry Pi kiosk (linuxkms, Perform, controller browsing) | DONE (beyond the usual) | Built and driven headless on CI (`packaging/ci/headless-e2e.sh`); no cargo test; (SPEC §3.3) | — |  |
| Keyboard shortcuts for layouts and a View menu | MISSING | (UX §1.9, §4 P3.7) | — | P3 |

## 13. DMX lighting

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| DMX outputs: FLX10 DMX port, Enttec USB-DMX, Art-Net, sACN, several universes | MISSING | (SPEC §10.5) | — | P2 |
| Fixture library (OFL), patch, groups, moving heads / bars / PARs / strobes / lasers / fog | MISSING | (SPEC §10.5) | — | P2 |
| Automatic light show from beat, phrase, energy and the mix (crossfade-aware) | MISSING | (SPEC §10.5) | — | P2 |
| Easy manual mode: scene pads, dimmer, strobe, blackout, tap/sync, controller pad mode | MISSING | (SPEC §10.5) | — | P2 |
| Pro mode: programmer, effects engine, cue stacks, per-track lighting cues, MIDI mapping | MISSING | (SPEC §10.5) | — | P2 |
| LIGHTING tab, split view in Perform, detachable window, 2D visualiser | MISSING | (SPEC §10.5) | — | P2 |

## 14. Video, DVS, streaming, cloud

| Feature | Status | Reason |
|---|---|---|
| Video / lyric | OUT-OF-SCOPE | SPEC §1: video; Video and lyric display |
| DVS | OUT-OF-SCOPE | SPEC §1: dvs |
| Streaming services | OUT-OF-SCOPE | SPEC §1: streaming; Beatport, Beatsource, SoundCloud, Spotify, TIDAL, Apple Music sources |
| Cloud library sync / Dropbox / mobile app sync | OUT-OF-SCOPE | SPEC §1: cloud; offline-first, no accounts (SPEC §2) |
| Live streaming / mix upload | OUT-OF-SCOPE | SPEC §1: cloud; needs online accounts; SPEC §2 offline-first |

---

## Counts

| Status | Rows |
|---|---|
| DONE | 83 |
| PARTIAL | 30 |
| ENGINE-ONLY | 27 |
| MISSING | 66 |
| deferred (in scope, later milestone) | 2 |
| OUT-OF-SCOPE | 5 |
| **Total** | **213** |

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
