<!-- Generated from tests/parity/features.toml by `cargo run -p od-parity --bin parity -- matrix`. Do not edit by hand. -->

# Feature matrix (OpenDecks Session)

Snapshot: 2026-11-06, from a read of the source tree.

**Summary:** 214 features; 202 in scope: 201 DONE (100 %), 0 PARTIAL, 0 ENGINE-ONLY, 1 MISSING; 7 deferred to a later milestone; 5 out of scope (SPEC §1).

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
| Column choice (album, label, comment, colour, date added, play count, bitrate, My Tag, …) | DONE | `browser/columns.rs`, `ui/table.slint`; Header right-click or the columns button opens the column menu (preview, title, artist, album, genre, label, comment, My Tag, colour, BPM, key, time, year, date added, play count, bitrate, rating); the choice is stored in the settings (`browser.columns`); "Default columns" resets | 3 | P2 |
| Artwork in browser / deck | DONE | `crates/od-ui/src/browser/mod.rs::artwork_image`, `ui/info.slint`; Stored (`schema.sql` `artwork`) and exported to sticks (`pdb/rows.rs`); shown as a thumbnail in the info panel and next to the title of the Prepare player (deck artwork is part of the deck header) | 1 | P3 |
| Tree: folders, playlists, smart lists, sets | DONE | `nodes.rs::create_node/rename_node/delete_node`; `app.rs` `on_create_node`, `on_rename_node`, `on_delete_node` | 2 |  |
| Reorder tree nodes (drag) | DONE | `nodes.rs::move_node`, `browser/mod.rs::node_dropped`; Drag a list or folder in the tree onto a folder (moves into it) or onto another list (moves before it); undoable | 2 | P2 |
| Add tracks to playlist | DONE | `app.rs` `on_add_to_node` | 1 |  |
| Reorder playlist / set entries | DONE | `nodes.rs::move_entries`, `browser/mod.rs::reorder_rows`; Drag rows (with the selection) between rows of a playlist or set sorted by #; one undo step | 1 | **P1** |
| Drag and drop (table → tree, Finder → library) | DONE | `ui/app.slint`, `crates/od-ui/src/import.rs::files_dropped`, `ui/drag.slint`; Rows drag onto decks, the Prepare players, playlists / sets / the tag list in the tree and between rows of a playlist; tree nodes drag onto folders. Files and folders from Finder / the file manager: dropped on the window they join the collection and the open playlist or set; in Perform a file dropped on a deck loads there. Slint's `DropArea` takes OS drops where the backend delivers them (UI test via the testing backend); the winit backend of Slint 1.18 does not yet, so `import::wire_native_drop` also takes winit `DroppedFile` events (no drop position there: collection and open list) | 4 | **P1** |
| Search | DONE | `tracks.rs::search`; `app.rs` `on_search` | 2 |  |
| Filter (BPM range, key, rating, My Tag, relative to master deck) | DONE | `browser/filter.rs`, `ui/filter.slint`; Filter bar under the header (filter button, ⌘J): BPM range, key compatible with the master deck, BPM within ±% of the master deck (double / half time count; without a loaded deck the selected track is the reference), minimum rating, one My Tag value; CLEAR. Smart lists keep the full rule language | 4 | P2 |
| Rating (stars) | DONE | `tracks.rs::set_tags`; Five cells per row, click to rate (click again clears); also in the row context menu. Verified by clicking in the running app | 1 | **P1** |
| Track colour | DONE | `od-core/src/track.rs`; Colour tag bar per row; set via the row context menu (eight colours, none) | 1 | **P1** |
| Comments | DONE | Edited in the track info panel (INFO), saved with Return as one undo step. Verified in the running app | 1 | **P1** |
| Edit tags (title, artist, album, genre, label, year …) | DONE | `tracks.rs::set_tags`; Track info panel edits title, artist, album, genre, label, remixer, composer, year, comment | 2 | **P1** |
| My Tag | DONE | `browser/sources.rs`, `ui/mytag.slint`; MY TAG panel: type "Category: Value" to tag the selected tracks, click a value to tag / untag them (undoable); MY TAG column; My Tag filter in the filter bar. The rekordbox importer still skips My Tag smart conditions | 3 | P2 |
| Tag list / palette (holding area) | DONE | `browser/sources.rs`, `ui/sidebar.slint`; TAG LIST in the tree: collect tracks with + (⌥⌘T), the row menu or by dragging rows onto it; Delete removes, SAVE AS PLAYLIST makes a playlist, CLEAR empties; kept in the library | 2 | P3 |
| Related tracks | DONE | `sets.rs::suggest_next`, `browser/sources.rs`; RELATED in the tree: tracks within ±6 % tempo and a compatible key after the master deck or deck 1–4 (selector in the title row), or after the selected track (row menu "Show related tracks"); FLX10 `browse.related` is still not mapped | 2 | P2 |
| Track suggestion / harmonic matching | DONE | `ui/mytag.slint::SuggestionPanel`, `browser/sources.rs`; RELATED panel: what fits after the selected track (tempo ±6 %, compatible key), double-click loads into a free deck, SHOW AS LIST opens it as the Related list; key-distance marks in the KEY column | 3 | P2 |
| Intelligent (smart) playlists | DONE | `od-library/src/smart.rs`, text rule editor `od-ui/src/smart_text.rs`, `on_set_smart_rule` | 4 |  |
| Histories: player histories from sticks | DONE | `od-devices/src/pioneer.rs` history proposals → `app.rs::resolve_proposal` creates a "History" folder | — |  |
| Histories: own play history | DONE | `recorder.rs::log_play`, `browser/sources.rs`; Every track that becomes the audible deck is logged in the run's session ("Session 2026-11-07 21:30"), recording or not; HISTORIES in the tree lists the sessions, each opens its plays in order | 2 | P2 |
| Track preview (Prepare) | DONE | `perform.rs::toggle_preview` on a preview deck; Space key | — |  |
| Loaded-on-deck indicator | DONE | `TrackRow.deck` | — |  |
| Duplicate track check | DONE | `crates/od-ui/src/browser/sources.rs::duplicates_action`; File ▸ Find Duplicate Tracks (⌥⌘Y) lists the groups in the browser; REMOVE SELECTED and KEEP ONE OF EACH remove copies from the collection (files stay on disk) | 3 |  |
| Import folder / files | DONE | `app.rs` `on_import_folder`, `od-cli import` | — |  |
| rekordbox library + stick migration | DONE | `od-import-rekordbox`, `od-ui/src/import.rs` | 7 |  |
| rekordbox preferences and keyboard shortcuts migration | DONE | `crates/od-import-rekordbox/src/settings.rs`, `crates/od-ui/src/import.rs::import_rekordbox_settings`; File ▸ Import rekordbox Preferences and Shortcuts and Preferences > Advanced: view, analysis, recording, deck and mixer values are applied; shortcuts are taken where they do not collide; `od-cli rekordbox-settings` reports the mapping; (SPEC §7) | 5 |  |
| rekordbox XML / iTunes library import-export (compatibility) | DONE | `crates/od-ui/src/import.rs::export_xml`, `crates/od-ui/src/import.rs::import_xml`; File ▸ Import / Export Library XML (rekordbox-compatible), also `od-cli xml-export` / `xml-import`; File ▸ Import Music / iTunes Library XML reads a `Library.xml` export (tracks with tags and rating, playlist folders; built-in lists skipped) | 6 | P3 |
| Missing files / relocate | DONE | `relocate.rs::check_missing/find_relocations`; `on_find_moved_files` | 1 |  |
| Consolidate | DONE | `relocate.rs::consolidate`, `crates/od-ui/src/import.rs::consolidate`; File ▸ Consolidate Library copies every track into <folder>/<Artist>/ in the background and points the library at the copies; originals stay | 2 |  |
| Undo/redo with history | DONE (beyond the usual) | `journal.rs`; Cmd-Z / Shift-Cmd-Z in `app.rs::key` | 3 |  |
| Sets with transition notes and flow view | DONE (own) | `sets.rs::set_flow`, `on_set_transition_note`, `ui/flow.slint` | 3 |  |
| Set backup tracks / planned start | DONE | `nodes.rs::set_entry_backups/set_set_meta`, `crates/od-ui/src/browser/mod.rs::set_backups`; Set view: planned start field; backups of the selected position are taken from the tag list (BACKUPS FROM TAG LIST) or cleared | 1 |  |
| Keyboard shortcuts for Prepare | DONE | `keymap.rs::registry`, `commands.rs::run`, `docs/keyboard.md`; One command registry (177 actions, conventional DJ-software defaults for decks 1/2); Preferences > Keyboard rebinds with conflict detection, reset, TOML import/export | 4 | P2 |
| Browse from controller | DONE | `crates/od-ui/src/browser/navigate.rs`; `browser::navigate`: BACK moves the encoder into the tree, turning steps through the lists (⇧ by eight), a press opens a folder / steps into it or moves into the track list; there turning steps rows, a press loads into a free deck, ⇧-press adds to the tag list; VIEW toggles the full browser (Prepare) or the Perform browse layout; RELATED opens the related tracks. The tree marks the encoder focus | 3 | P2 |
| Explorer: browse folders on disk and play directly | DONE | `crates/od-ui/src/explorer.rs::ExplorerState::rows`, `crates/od-ui/src/explorer.rs::open_folder`, `crates/od-ui/ui/sidebar.slint::ExplorerItem`; User-added roots (persisted in library meta) as a nested folder tree of any depth: sub-folders are listed lazily on expand/select, each probed one level so only expandable folders get a mark; hidden and player data folders are skipped. Selecting a folder expands it and lists its audio files; (SPEC §5.5; UX §2.2) | 5 |  |
| Explorer: "Include subfolders" lists the files of a folder and all its sub-folders | DONE (beyond rekordbox) | `crates/od-ui/src/explorer.rs::list_files`, `crates/od-ui/src/explorer.rs::set_recursive`, `crates/od-ui/ui/app.slint`; Toggle in the folder list header (persisted as `explorer.recursive`); rekordbox lists only the folder itself. Depth-first, each folder by name, symlink loops followed once, capped at 32 levels / 20 000 files; (SPEC §5.5) | 3 |  |
| Explorer: loading a file adds it to the collection and analyses it in the background | DONE | `crates/od-ui/src/explorer.rs::start_import`, `crates/od-ui/src/explorer.rs::poll`; Loading a file from the Explorer adds it to the collection in the background (`workers::import_file`) and loads it; the flow test asserts the deck and the collection; (SPEC §5.5) | 1 |  |
| Source rail switching the tree (Collection, Playlists, Related, Explorer, Devices, Histories, Recordings) | DONE | `crates/od-ui/ui/sidebar.slint`; 38 px rail left of the tree in the familiar source order (Collection, Playlists, Related, Sampler, Explorer, Devices, Histories, Recordings, Tag list): each opens its source or scrolls the tree to it and marks the open one; Sampler opens the sampler panel in Perform; (UX §1.6, §4 P4.9) | 1 | P2 |
| Re-orderable columns and multi-sort | DONE | `browser/columns.rs`, `browser/tracks.rs::sort_rows`; Drag a header cell onto another column to move it there; ⇧-click adds (or reverses) up to three secondary sort columns, shown with their rank; order and secondary sort persist in the settings; (SPEC §5.5) | 3 | P3 |
| Load a track: drag to deck, double-click (Prepare), context menu Load ▸ Deck n | DONE | `crates/od-ui/src/decks.rs::free_deck`; Double-click / activate loads into a free deck, drag a row onto a deck, context menu Load to deck ▸ Deck n, controller load; all driven through the window by UI tests; (UX §2.1) | 3 |  |
| Drag tracks from the browser (and Explorer, Finder) onto a deck or the Prepare player | DONE | `crates/od-ui/src/import.rs::files_dropped`, `crates/od-ui/ui/app.slint`; List, Explorer and sub-browser rows and files from Finder / the file manager onto any deck (1–4) or either Prepare player, with drag ghost and the target highlighting while dragging; files dropped on a list in the tree go into that list, elsewhere into the collection and the open list (at the row under the pointer when it is in its own order). UI-tested on the Slint drop path. Native winit drops carry no position: on macOS the pointer is read from AppKit while files hover; on Linux they go to the collection and the open list; (SPEC §5.5) | 5 | **P1** |
| Track row context menu (Load ▸, Add to playlist ▸, Analyse, Show in Finder, …) | DONE | Right-click a row: Load to deck ▸ 1-4, Add to playlist ▸, Colour ▸, Rating ▸, Analyse, Show in file manager, Remove from this list. UI tests drive the real menu (rating, colour, remove, load). Further items of the layout reference (tag list, export, play count, hot-cue tools, info) are tracked by the view test `ctx.track-row`; (UX §1.9) | 3 |  |
| Info panel (track details beside the list) | DONE | INFO in the list title row opens the track info panel beside the list; every tag is editable in place (Return stores, undoable). File details (type, size, sample rate, bit rate) and the tabs of the layout reference are tracked by the view test `browser.info-panel`; (SPEC §5.5; UX §1.6) | 1 |  |
| Sub-browser (second tree + list for dragging between playlists) | DONE | `crates/od-ui/src/browser/sub.rs`, `crates/od-ui/ui/subbrowser.slint`; SUB in the list header, View > Sub Browser and a shortcut open a second tree (all library lists) and list under the main list; main-list rows dropped on its lists are added there, its rows go to main-tree lists, the open list, decks and players; (UX §1.6, §4 P4.12) | 2 | P3 |
| Automatic database backups (rotating) | DONE (beyond the usual) | `crates/od-library/src/backup.rs`; (SPEC §5.3) | 1 |  |
| Export / import the whole library as one archive (database + analysis, optional audio) | DONE | `crates/od-ui/src/import.rs::export_archive`, `crates/od-ui/src/import.rs::restore_archive`; File ▸ Export Library Archive (asks whether to include the audio; runs in the background) and Restore Library Archive (into a chosen folder, opened from the next start); (SPEC §5.3) | 2 |  |
| MCP server: full library access for preparing sets with an LLM | DONE | `apps/od-mcp/src/tools.rs`, `apps/od-mcp/src/server.rs`; Owner request 2026-11-06 (SPEC §5.6). `opendecks-mcp` (MCP 2025-06-18, stdio or loopback HTTP): 8 read and 10 write tools, track/node resources, build_set and fix_transitions prompts; each write is one undo step "MCP: …" plus change-log rows (stick sync); setup in docs/mcp.md; (SPEC §5.6) | 8 | **P1** |

## 2. Analysis

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| BPM + beatgrid incl. downbeats | DONE | `od-analysis/src/tempo.rs::detect_beatgrid`; background worker `od-ui/src/workers.rs` | 6 |  |
| Variable-tempo grid | DONE | `od-core/src/beatgrid.rs`, `crates/od-ui/src/player.rs::tempo_marker`; Grid tools: + MARKER sets the typed BPM from the beat at the cursor on (beats before keep their tempo, the bar count carries on); several markers make a variable-tempo grid | 3 | P3 |
| Grid editor: shift, ×2/÷2, set downbeat, lock | DONE | `app.rs` `on_grid_shift/on_grid_scale/on_grid_downbeat/on_grid_lock` | 3 |  |
| Grid editor: tap tempo, type BPM | DONE | `crates/od-ui/src/player.rs::tap`, `crates/od-ui/src/player.rs::type_bpm`; GRID tab: BPM field (Return sets a constant grid at that tempo, keeping the first downbeat) and TAP (from the fourth tap; key B); a locked grid is not changed | 4 |  |
| BPM analysis range setting | DONE | Preferences ▸ Analysis BPM range (`preferences.rs` `bpm_range`) feeds the tempo analysis fold range | 1 | P3 |
| Key detection, notation Camelot/Open Key/musical | DONE | `od-analysis/src/key.rs`; `--keys` flag, `KeyNotation` | 7 |  |
| Phrase analysis | DONE | Analysed (`od-analysis/src/phrase.rs`), stored and exported (`PSSI` in `anlz/build.rs`). Display: see `deck-phrase-display` | 2 |  |
| Waveform: own 3-band colour | DONE | `od-analysis/src/waveform.rs`, `render.rs` (+ colour-blind palette) | 4 |  |
| Waveform style options (RGB / blue / 3-band) | DONE | `PWAV/PWV2–PWV7`; Preferences ▸ View ▸ Waveform colour: 3-band (low / mid / high stacked), blue, or RGB (one envelope, hue from the band balance) | 2 | P3 |
| Vocal position | DONE | Derived from the separated vocal part when a track has parts: spans where the vocals sing are drawn as a band along the top of the deck overview | 2 | P3 |
| Loudness + auto gain | DONE | `od-analysis/src/loudness.rs`; gain in `perform.rs::load_track`, engine default on (`mixer.rs` `auto_gain: true`); no toggle in UI | 3 |  |
| Re-analyse selected, never overwrite manual/imported | DONE | `on_analyse_selected`; `DataSource` priority in `tracks.rs::store_analysis` | 3 |  |
| Phrase editing (GRID EDIT \| PHRASE EDIT, cut / clear) | DONE | `crates/od-ui/src/phrase_edit.rs`, `crates/od-library/src/tracks.rs::set_phrases`; PHRASE tab next to GRID in the Prepare player: kind of the phrase at the cursor (a gap gets a new phrase), CUT at the nearest bar (at least 4 bars each side), BOUNDARY TO CURSOR, CLEAR; keyboard commands. Stored with Library::set_phrases (normalized with od_core::normalize_phrases, one undo step, change log → sticks resync). A new analysis of the track replaces edited phrases; (UX §2.5) | 6 | P3 |

## 3. Cues and loops

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 8 hot cues A–H (Prepare) | DONE | `app.rs` `on_hot_cue/on_remove_hot_cue`; keys 1–8 | 2 |  |
| 8 hot cues A–H (Perform, pads/UI) | DONE | `DeckCommand::HotCue`, `od-ui/src/decks.rs::persist_cue_events`; Set/delete on pads or the controller are saved to the library (one undo step each) and reach sticks through the normal sync | 2 | **P1** |
| Hot cue banks / hot cue bank lists | DONE | `pads.rs::bank_label`; Banks A–H, I–P, Q–X, Y–AF with ‹ › in Hot Cue mode (keyboard too); pads show the bank's letters and labels | 2 | P3 |
| Memory cues: add (Prepare) | DONE | `on_add_memory_cue` | — |  |
| Memory cues: delete, move (Prepare) | DONE | Cue list (ALL / MEMORY / HOT CUE): select a cue to edit it: name, colour, move to the cursor (beat-snapped, loops keep their length), make a memory cue a hot cue, delete | 2 | **P1** |
| Memory cues: call prev/next (Perform) | DONE | `MemoryCueJump` via controller | — |  |
| Memory cue set/delete from controller | DONE | `decks.rs::memory_cue`; `deck.memory_cue_set` / `_delete` edit the library cue list (one undo step) and update the deck without a gap | 1 | **P1** |
| Saved loops (memory/hot-cue loops) | DONE | Loop hot cues and memory loops (MEMORY while a loop plays) are stored with their loop end; the Prepare cue editor moves loops keeping their length; import / export / playback carry them | 3 | **P1** |
| Active loop (auto-engaged saved loop) | DONE | ACTIVE in the cue editor marks a saved loop; it is stored (library schema 3) and engages by itself when the playhead reaches it (`DeckCommand::SetActiveLoop` on load) | 3 | P3 |
| Cue colours | DONE | Eight cue colours in the cue editor; shown on overviews, the cue list, Prepare pads and the Perform hot cue pads | 3 | **P1** |
| Cue comments / labels | DONE | Double-click a cue in the list, type the name, Return; names show on pads and in the list | 2 | **P1** |
| Auto cue on load (first memory cue / first beat) | DONE | `DeckCommand::Load` behaviour (`command.rs` doc, `deck.rs`) | — |  |
| Cues set on players → library | DONE (beyond the usual) | `od-devices/src/pioneer.rs` proposals, `on_accept_proposal` | 2 |  |

## 4. Deck / player

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 2 / 4 decks, deck layers | DONE | `on_set_deck_count`, FLX10 auto 4 decks (`perform.rs::frame`), `DeckSelect` in `od-controller/src/session.rs` | 3 |  |
| Play / Cue (CDJ cue logic) | DONE | `DeckCommand::Cue`; UI + controller | 2 |  |
| Jog: vinyl scratch, bend, search; vinyl mode toggle | DONE | `DeckCommand::JogTurn/JogTouch/SetJogMode`, `handle_action` | 2 |  |
| Jog display (FLX10 screens) | MISSING, deferred to M4 | Needs the non-MIDI protocol (SPEC M4, `docs/ddj-flx10-usb-protocol.md`) (M4) | — | P3 |
| Tempo fader, ranges ±6/10/16/WIDE, reset | DONE | `TempoRange`; controller cycles range; UI shows it but has no range button | 3 |  |
| Master tempo / key lock (Rubber Band) | DONE | `od-engine/src/keylock`, UI toggle + controller | 4 |  |
| Key shift (±semitones) | DONE | `decks.rs::key_shift`; ‹ ±0 › in the deck header (click resets), Key Shift pad mode, keyboard | 2 | P2 |
| Key sync | DONE | `decks.rs::toggle_key_sync`; KEY SYNC in the deck header, keyboard and controller (`deck.key_sync` with LED); the track key reaches the engine on load | 1 | P2 |
| Beat sync + master deck | DONE | `SetSync`, `SetMasterDeck`; UI + controller | 1 |  |
| Quantize | DONE | `Command::SetQuantize`; UI + controller | 1 |  |
| Slip | DONE | `SetSlip`; UI + controller | 1 |  |
| Reverse | DONE | `SetReverse` via controller; no UI button | 2 |  |
| Censor (slip reverse) | DONE | `DeckAction::Censor` in `handle_action` | — |  |
| Beat jump | DONE | `DeckCommand::BeatJump`; UI ±4, controller, pad mode | 1 |  |
| Beat jump size setting | DONE | `decks.rs::step_jump_size`; ‹ › in Beat Jump pad mode (1/8 … 64); the deck's −/+ buttons and the keyboard jump by it | 1 | P3 |
| Manual loop in/out, reloop/exit, ½ / 2× | DONE | `LoopIn/LoopOut/Reloop/LoopExit/LoopHalve/LoopDouble` | 1 |  |
| Auto beat loop (1/32 … 32) | DONE | `DeckCommand::AutoLoop`; UI size stepper | 1 |  |
| Loop roll | DONE | `DeckCommand::LoopRoll` via controller | 1 |  |
| Needle search | DONE | `NeedleSearch`; overview click + FLX touch strip | 1 |  |
| Search (FF/REW buttons) | DONE | ◀◀ / ▶▶ on the player screens search while held (`DeckCommand::Search`) | 2 | **P1** |
| Tap BPM | DONE | `decks.rs::tap_bpm`; Controller TAP: three taps give a constant grid at the tapped tempo, keeping the first downbeat (saved, undoable; locked grids stay) | 2 | **P1** |
| Pad mode: Hot Cue | DONE | `PadMode::HotCue` in `handle_action` | 1 |  |
| Pad mode: Beat Loop | DONE | `PadMode::BeatLoop` → `AutoLoop` | 2 |  |
| Pad mode: Beat Jump | DONE | `PadMode::BeatJump` | 2 |  |
| Pad mode: Sampler | DONE | `pads.rs::sampler_slot`; Odd decks play pads 1–8, even decks 9–16 of the shown bank; gate samples follow the pad | 1 | P2 |
| Pad mode: Pad FX 1/2 | DONE | `pads.rs::press_commands`; Pad FX page 1 / 2 (‹ ›), momentary | 2 | P2 |
| Pad mode: Keyboard | DONE | `pads.rs::KEY_STEPS`; −3 … +4 semitones on the chosen hot cue (‹ CUE A ›) | 2 | P3 |
| Pad mode: Key Shift | DONE | `pads.rs::press_commands`; Pads set the key shift −3 … +4 | 1 | P2 |
| Pad mode: Slicer | DONE | `pads.rs::mode_change_commands`; `PadMode::Slicer`; domain 4 … 64 beats (‹ ›), starts and stops with the mode | 2 | P3 |
| Pad LED feedback per mode | DONE | `controller.rs::feedback`; Pad mode LEDs and the pads of the current mode (on = engaged, dim = loaded / set) | 2 | P2 |
| Waveform zoom in Perform | DONE | `perform.rs`; Mouse wheel over a lane zooms that deck, double-click resets (`decks::zoom_by`); zoom buttons of the zone column are tracked by the view test (`wave.zoom-*`) | 1 |  |
| Waveform overview + scrolling detail, cue markers, beat ticks | DONE | `render.rs::overview/detail`, `ui/plate.slint` | 3 |  |
| Phrases on the overview and enlarged waveform, with display options | DONE | `crates/od-ui/src/player.rs::phrase_marks`, `crates/od-ui/src/render.rs`, `crates/od-ui/src/settings.rs::PHRASE_TYPES`; Phrases on the enlarged waveforms and the phrase bar under the overviews switch separately (Preferences > View), phrase types as names or colours only; Prepare player and Perform decks; (SPEC §10.1; UX §2.5) | 3 | P2 |
| Deck header: title, artist, artwork, BPM, remaining / elapsed time | DONE | `crates/od-ui/src/format.rs`; Number, cover art (from the library, as a thumbnail), title, artist, BPM, remaining / elapsed time, key, SYNC / MASTER; (SPEC §10.1; UX §1.3) | 5 | P2 |
| Jog / platter readout in the UI: position, remaining, phrase and bar counter | DONE | Jog display: bar counter (bars.beats, count-down modes), remaining time, phrase with its countdown; (SPEC §10.1; UX §1.3) | — | P2 |
| Full-width enlarged waveform zone, one stacked lane per deck (2 and 4 decks), centre playhead | DONE | `crates/od-ui/src/decks.rs`; One full-width lane per deck (2 and 4 decks), stacked, playhead at the centre. The zone column (collapse / zoom buttons / expand) is tracked by the view test; (SPEC §10.1; UX §1.3, §4 P1.1) | 1 |  |
| Pad mode: Stems (part mutes and stem FX on pads) | DONE | (UX §2.4) | 2 | P2 |
| Pad mode: Memory Cue | DONE | (UX §1.3) | 2 | P3 |
| Pad mode: Sequence Call | DONE | Pad mode SEQ CALL: an empty pad stores the sampler sequence patterns, a stored pad recalls them and starts the sequencer, the shifted pad clears it; (UX §1.3) | 1 | P3 |
| Pad mode: Active Censor | DONE | Pad mode CENSOR: each pad censors (reverse with slip) for ¼ … 32 beats of the deck tempo, then playback continues where it would be; (UX §1.3) | 2 | P3 |
| CDJ practice mode: CDJ-3000-like screens and behaviour driven by the controller | DONE | CDJ toggle (Alt+J): player screens per deck (waveform, overview needle search, jog display with remaining time / bar counter / phrase, hot cues with hold, memory cue call, loop in / out / reloop, beat jump, search, tempo range, master tempo), BROWSE screen (source → playlists → tracks, from the library or a stick), player cue rules, sync off, self-ticking checklist; (SPEC §10.3c) | 4 | P2 |

## 5. Mixer

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Trim | DONE | `ChannelCommand::SetTrim`; UI knob + controller | 2 |  |
| 3-band EQ (isolator) | DONE | `dsp/eq.rs` default `Isolator`; UI + controller | 3 |  |
| EQ mode classic/isolator | DONE | `ChannelCommand::SetEqMode`, `crates/od-ui/src/preferences.rs::mixer_settings`; Preferences > Controller > Mixer: isolator / classic EQ, stored and applied live and at engine start | 2 |  |
| Colour FX knob (filter) | DONE | `SetColorFxAmount`; UI + controller (default kind = filter) | 1 |  |
| Channel fader | DONE | UI + controller | 3 |  |
| Channel fader curve | DONE | `crates/od-ui/src/preferences.rs::mixer_settings`; Preferences > Controller > Mixer: linear / smooth / sharp | 3 |  |
| Crossfader | DONE | UI + controller | 2 |  |
| Crossfader curve | DONE | `crates/od-ui/src/preferences.rs::mixer_settings`; Preferences > Controller > Mixer: constant power / linear / cut | 3 |  |
| Crossfader assign A/THRU/B | DONE | Controller (`MixerAction::CrossfaderAssign`); no UI control | 1 |  |
| Headphone cue, mix, level | DONE | `SetCue`, `SetHeadphoneMix/Level`; UI + controller | 2 |  |
| Master level, VU meters, limiter | DONE | `SetMasterLevel`, meters in `ChannelView`, limiter on by default | 3 |  |
| Master cue | DONE | `controller.rs::handle_action`; Controller `master_cue` toggles it, with LED | 1 | P2 |
| Booth level / booth output | DONE | `controller.rs::handle_action`; Controller `booth_level` sets the booth output | 1 | P2 |
| Smart CFX / Smart Fader (FLX4) | DONE | Smart CFX: a colour FX on one knob (low-pass + echo left, high-pass + reverb right); Smart Fader (SMART beside the crossfader): the crossfader swaps the basses of the A and B channels and the decks on the crossfader follow the master tempo | 2 | P3 |
| Mic / aux input | DONE | MIC in the global bar: on, level, input meter, talkover with ducking indicator, input device; engine input channel mixed before Beat FX / booth / recording | 5 | P3 |
| Auto gain toggle | DONE | `ChannelCommand::SetAutoGain`; Preferences > Controller > Mixer: auto gain on / off for every channel | 2 |  |
| STEM button per mixer channel | DONE | STEM per mixer channel switches its HI / MID / LOW knobs to part isolation (vocals / inst / drums), centre = full level; the channel EQ stays as it was; (UX §1.3, §2.4) | 1 | P2 |

## 6. Effects

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Beat FX (2 units: echo, delay, reverb, flanger, phaser, roll, filter sweep, trans) | DONE | `crates/od-ui/src/fx.rs`, `crates/od-ui/ui/fx.slint`; FX panel (top bar FX) with two units; controller `beat_fx.*` inputs drive the same functions; ON / release LEDs | 4 | **P1** |
| Beat FX: beat select, auto beat, tap, channel assign | DONE | `fx.rs::step_beats`, `fx.rs::TapTempo`; Beat ‹ › (1/8 … 16), TAP (three taps) and AUTO (back to the deck tempo), channel assign 1–4 / master, level and parameter knobs; on screen, keyboard and controller | 4 | **P1** |
| Larger Beat FX list (ping pong, spiral, helix, pitch, vinyl brake, triplet, shimmer …) | DONE | `command.rs::BeatFxKind::ALL`; 14 effects selectable in the panel (list or ‹ ›); controller effect names map to the closest one | 2 | P3 |
| Sound Colour FX selection (filter, noise, space, crush, echo) | DONE | `fx.rs::select_color_fx`; One global selection (‹ NAME › under the mixer, keyboard, controller CFX buttons with LEDs), persisted, applied to every channel | 3 | **P1** |
| Release FX | DONE | `fx.slint::FxUnit`; BRAKE / BACKSPIN / ECHO OUT per unit while held (panel, keyboard); controller release button = echo out | 2 | P2 |
| Pad FX | DONE | `fx.rs::pad_fx_preset`; Two pages of eight presets (beat and release FX) on the deck pads | 3 | P2 |
| Stem FX (FX on one part) | MISSING | Not yet: FX on one part needs that part as its own time-stretched signal (a second keylock path per deck). Planned with the keylock refactor; part levels, part isolation and Beat FX on the whole channel work today | — | P3 |
| FX panel (FX1 \| FX2 with three slots, beat value, depth, assign, AUTO, TAP; CFX row) | DONE | `crates/od-ui/ui/fx.slint`; FX1 \| FX2: effect, beat, assign (channels, sampler, master), level, parameter, ON, AUTO, TAP, Release FX, and ×3 multi mode with three chained slots (effect, beat, depth, ON each); (SPEC §10.2; UX §1.5) | 4 | **P1** |
| Colour FX: pitch, dub echo, sweep | DONE | `settings.rs::COLOR_FX`; Pitch, dub echo and sweep selectable like the others; (SPEC §10.2) | 2 | P3 |

## 7. Stems / track separation

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Offline separation (vocals, drums, bass, inst), cache, background | DONE | `od-stems` (`worker.rs`, `cache.rs`), `perform.rs::request_stems/poll_stems` | 5 |  |
| Part mute toggles in UI | DONE | `on_deck_stem` → `SetStemLevel` | 2 |  |
| Stem levels / part isolator knobs | DONE | Part keys on each deck: click mutes, the wheel sets the level (level line under the key); FLX10 part isolator knobs drive the same levels (mapping marked verify until checked on hardware) | 2 | **P1** |
| Stem buttons on the controller | ENGINE-ONLY, deferred to M4 | `flx10.toml`; `DeckAction::StemMuteToggle` and the mapping action exist; which FLX10 notes are the part buttons (vs. mix point buttons) needs the USB/MIDI capture with the controller (M4, SPEC §9.3) | — | **P1** |
| Separate ahead of time (Prepare, batch) | DONE | `crates/od-ui/src/stems_batch.rs`; Row menu Separate parts and ⌥⌘⇧S queue the selected tracks on a background separation worker writing into the decks' stem cache; the status line reports progress and the result. Also `od-cli stems` | 2 | P2 |
| Separation model downloaded once on demand, nothing leaves the machine | DONE | `crates/od-stems/src/model.rs`; (SPEC §10.3) | 1 |  |
| Deck shows separation progress; stem controls visible (dimmed) while separating | DONE | Separation is queued on load; the stem strip shows SPLIT and the progress in percent; the part keys stay in place, disabled until parts exist (flow test); (SPEC §10.3; UX §2.4) | 2 | P2 |
| Raspberry Pi plays parts separated on the desktop | DONE | The native sync copies cached parts to /OPENDECKS/stems on the stick (and prunes stale ones); tracks loaded from a stick read their parts from there, so the Pi plays parts separated on the desktop; (SPEC §10.3) | 2 | P2 |

## 8. Sampler

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| 16 slots, one-shot/loop, gain, stop | DONE | `crates/od-ui/src/sampler.rs`; 16 pads per bank in the sampler panel: play, stop all, clear, gain | 3 | P2 |
| Load samples / sample library | DONE | `sampler.rs::load_selected`, `sampler.slint::SampleDrop`; LOAD SELECTED or drop a track row on a pad; decoded off the UI thread; slot contents kept in the library meta and restored when audio starts | 2 | P2 |
| Banks (bank next/prev) | DONE | `sampler.rs::step_bank`; Four banks (panel, keyboard, controller `sampler.bank_*`) | 1 | P2 |
| Sampler volume, cue | DONE | `sampler.rs::set_volume`; VOL knob and CUE in the panel; controller `sampler.volume` / `sampler.cue` with LED | 2 | P2 |
| Sequencer | DONE | `sampler.slint::SamplerPanel`; 16 steps of the selected slot under the pads, PLAY / REC / CLR, playing step shown; patterns saved with the slots | 4 | P3 |
| Sampler panel (2 x 8 slots, gain, cue, BPM, bank row) | DONE | `crates/od-ui/ui/sampler.slint`; Between the global bar and the browser (top bar SAMPLER): bank, 2 × 8 pads, selected slot (mode, sync, BPM, gain), quantise, cue, volume; (SPEC §10.3a; UX §1.5) | 2 | P2 |
| Gate mode, quantised triggering, sample sync to the master tempo | DONE | `sampler.slint::SamplerPanel`; 1-SHOT / LOOP / GATE (pads report press and release), QUANT, SYNC per slot; (SPEC §10.3a) | 4 | P2 |

## 9. Recording

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| One-button mix recording (UI + controller) | DONE | `perform.rs::toggle_recording`, `ui/topbar.slint`, `RecordingToggle` + LED | 2 |  |
| WAV output | DONE | `od-recorder/src/lib.rs` (hound) | 1 |  |
| FLAC / MP3 output | DONE | WAV recorded, then encoded to FLAC (bit-exact) or MP3 320k when recording stops; format chosen in Preferences ▸ Recording | 1 | P2 |
| Automatic tracklist (.cue + text) | DONE | `Recorder::mark_track`, `cue_sheet`, `text_tracklist` | 2 |  |
| Split into per-track files | DONE | `od_recorder::split`, `on_split_recording` | 1 |  |
| Recordings section in the library | DONE | `recording` table, `refresh_recordings`, rail "RECORDINGS" | 1 |  |
| Silence auto start/stop | DONE | `crates/od-ui/src/recorder.rs::auto_step`; `od_recorder::SilenceDetector` drives the recorder: start after 2 s of sound (Preferences > Recording > Start), optional stop after 30 s of silence (> Stop); a stop by hand disarms | 3 |  |
| Output folder / file name setting, recording level | DONE | `crates/od-ui/src/recorder.rs::recording_name`, `crates/od-ui/src/recorder.rs::apply_gain`; Preferences > Recording: folder, file name pattern ({date}, {time}, {n}), recording level −12…+12 dB (also on the recording panel) | 3 |  |
| Recording panel: source, level, meter, elapsed time, file size, live tracklist | DONE | `crates/od-ui/src/recorder.rs::frame`, `crates/od-ui/ui/rec.slint`; Recording panel (Recordings section): REC, source, level, meter, elapsed time, file size, file name and the tracklist as it grows; `RecStrip` (source, level, meter) for the global bar; (SPEC §11; UX §2.3) | 3 |  |

## 10. Export / USB devices

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Legacy Pioneer export (`export.pdb` + ANLZ DAT/EXT/2EX) | DONE | `od-format-pioneer`, `od-devices/src/pioneer.rs` | 9 |  |
| Automatic incremental sync (Sync Manager equivalent) | DONE (beyond the usual) | `od-devices/src/sync.rs`, `manager.rs`; device sidebar | 4 |  |
| Device registry, formats, assigned content, rebuild, eject | DONE | `app.rs` `on_register_volume/on_set_device_*/on_rebuild_device/on_eject_device` | 4 |  |
| Browse / play from a stick | DONE | `on_open_device`, read-only library | 1 |  |
| OpenDecks native stick format | DONE | `od-format-native` | 3 |  |
| `exportExt.pdb` (My Tag on players) | MISSING, deferred to M6 | `pdb/mod.rs`; Not written. Public documentation (Deep Symmetry crate-digger rekordbox_pdb.ksy) describes only two of the nine exportExt.pdb table types: tags (3) and tag_tracks (4) rows, with fields it marks as unknown or constant ("seems always zero", the 0x03 byte, subtype 0x0680/0x0684 with conflicting notes on the far-name flag, tag_index in steps of 0x20). Table types 0–2 and 5–8, what players expect in them, and the ext page header values (the 1004 / page-flag 0x44 / 0x64 "strange pages") are undocumented; a file built from this would be guesswork without a player or a reference export to verify against | — | P3 |
| Player settings on stick (`MYSETTING`) | DONE | `od-format-pioneer/src/mysetting.rs`, `od-devices/src/pioneer.rs::player_settings`; `/PIONEER/MYSETTING.DAT` written on every Pioneer sync from the deck defaults (quantize, auto cue, tempo range, jog mode, language), layout as documented by pyrekordbox (104-byte header, 40-byte body, CRC-16/XMODEM over header and body); other settings get the documented defaults. MYSETTING2 / DJMMYSETTING are not written | 6 | P3 |
| Device Library Plus / OneLibrary | MISSING, deferred to M6 | SPEC §8.5 / §15 M6: OneLibrary (`exportLibrary.db`) export comes later; tracked, not counted | — | P3 |
| Pro DJ Link (network export) | MISSING, deferred to M6 | SPEC §10.3b "Pro DJ Link (later)", §15 M6; tracked, not counted | — | P3 |

## 11. Preferences, audio and controllers

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Preferences window | DONE | `ui/preferences.slint`, `preferences.rs::open`, `settings.rs::Settings`; Separate window (⌘, / Ctrl+, from the native menu and the keymap, gear in the top bar, Esc / ⌘W closes): Audio, View, Analysis, Controller, Keyboard, Recording, Advanced; stored in the library meta, CLI flags override for one run | 4 | **P1** |
| Audio device, buffer size, output routing | DONE | `engine.rs::audio_config`, `preferences.rs::refresh`; Preferences > Audio: output device (automatic = controller interface), buffer 64–1024, sample rate, headphones auto / 3-4 / off; applies when audio next starts | 2 | **P1** |
| Language, key notation, colour-blind, high contrast, motion | DONE | `preferences.rs::apply_live`, `shell.rs::apply_theme`; Preferences > View, applied live in both windows; UI scale (75–200 %) after restart | 3 | **P1** |
| Waveform options (zoom default, phrase display) | DONE | `settings.rs::ViewSettings`, `crates/od-ui/src/preferences.rs::default_deck_zoom`; Phrase display on/off (live, Prepare and Perform); waveform zoom default (4/8/16/32 s) for the Prepare player and the decks; waveform colour 3-band / blue / RGB read by the renderer | 3 | P2 |
| Deck defaults (tempo range, quantize, sync mode, jog mode, auto cue) | DONE | `crates/od-ui/src/preferences.rs::deck_defaults`, `crates/od-ui/src/preferences.rs::apply_engine_settings`; Preferences > Controller > Decks: tempo range, quantize, sync mode (beat or tempo only), jog mode and auto cue, stored and sent to every deck when audio starts and on change | 5 | P2 |
| Controller LED feedback | DONE | `perform.rs::feedback` (play, cue, sync, master, key lock, slip, loop, pads, VU) | 3 |  |
| Soft takeover | DONE | `od-controller/src/takeover.rs` | 4 |  |
| MIDI learn / MIDI monitor | DONE | `crates/od-ui/src/midi_tools.rs`, `od-controller/src/learn.rs`; Preferences ▸ Controller: MIDI monitor (raw messages with the mapping hits) and MIDI learn (action, deck, the learned control described and as a mapping entry; ADD TO MY MAPPING writes it to the own mapping file, checked to load) | 7 | P3 |
| FLX10 utility settings (jog ring, fader start, USB input …) | PARTIAL, deferred to M4 | `crates/od-ui/src/midi_tools.rs`, `od-controller/src/flx10_settings.rs`; Preferences ▸ Controller ▸ DDJ-FLX10: fader start, REC OUT, jog display time, slip flashing, SLIP button, hot cue indicator, jog ring colour, USB boost; stored, with the control transfers shown. WRITE TO DDJ-FLX10 sends them over USB only in builds with the `flx10-usb` feature (libusb, off by default); USB input routing has no UI yet | 5 | P3 |
| Audio output on macOS (CoreAudio) | DONE | `crates/od-audio-io/src/output.rs`; (SPEC §10.4) | 1 |  |
| Linux audio: PipeWire / JACK, ALSA direct on the Pi | DONE | `crates/od-audio-io/src/devices.rs`; ALSA (always; direct on the Pi), JACK and PipeWire (features `jack`, `pipewire`) selectable in Preferences ▸ Audio; CI builds and tests all three hosts on Linux x86_64 and aarch64; (SPEC §10.4) | 4 | P2 |
| DDJ-FLX10 mapping (4 decks, LEDs) | DONE | `mappings/pioneer/ddj-flx10.toml`; (SPEC §9.1) | 3 |  |
| DDJ-FLX4 mapping (2 decks, LEDs) | DONE | (SPEC §9.1) | 2 |  |
| Browser font size and line spacing | DONE | `crates/od-ui/src/shell.rs::apply_browser_style`; Preferences > View > Browser: text size 12–18 px and line spacing compact / normal / roomy (22 / 28 / 34 px rows), live; (UX §1.8, §4 P5.17) | 2 |  |

## 12. Shell, layouts and panels

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| Edit mode (track editing / remix) | DONE | `crates/od-ui/src/track_edit.rs`; Minimal edit mode (EDIT tab of the Prepare player): a region (IN / OUT at the cursor or shift-drag on the overview) is cut out, trimmed to or repeated with 5 ms crossfades at the joins; the result is a new 24-bit WAV next to the original, added as "<title> (Edit)" with the original tags and artwork and analysed; the original stays untouched. No multi-track arrangement | 5 | P3 |
| Prepare / Perform mode switch that keeps loaded tracks | DONE | `crates/od-ui/src/shell.rs`; (SPEC §3.1) | — |  |
| Perform layouts: 2 / 4 decks horizontal and vertical, Browse | DONE | 2 and 4 decks in three layouts (persisted): standard (lanes across the top), vertical (Alt+V: upright lanes with zoom keys above the mixer) and Browse (Alt+B: header, overview and parts per deck, browser enlarged); (SPEC §10.1; UX §1.4) | 5 | P2 |
| Prepare layouts: 1 player, 2 players, dual, simple, full browser | DONE | `crates/od-ui/src/shell.rs::set_prepare_layout`; Player and browser, 2 players (full player plus a compact second player), dual (two compact players side by side), simple (one compact player) and full browser: top-bar switch, View menu, ⌥⌘4–⌥⌘8, persisted. Player 2 takes the selected track (LOAD) or a dragged row and plays on the preview deck; (UX §1.7) | 3 | P2 |
| Top-bar panel toggles: FX, Mix Point Link, Sampler, Mixer, Recording | DONE | FX, MIX POINT, SAMPLER, MIXER and REC in the top bar (stored): MIXER hides the mixer column, MIX POINT shows the Mix Point Link panel, REC shows or hides the recording section, plus CDJ and LIGHT; (UX §1.2, §2.8) | 3 | P2 |
| Mix Point Link (mix in / mix out points between decks) | DONE | `crates/od-ui/src/mpl.rs`, `crates/od-ui/ui/mpl.slint`; MIX POINT panel above the waveforms: MIX IN / MIX OUT at the edited deck's playhead (stored per track), ◀ ▶ through the points (memory cues and mix points), EDIT switches the deck, LINK starts the waiting deck from its mix-in point when the playing deck passes its mix-out point; (UX §1.5; SPEC §16.3) | 5 | P3 |
| Draggable splitters with persisted sizes, double-click reset | DONE | `crates/od-ui/src/shell.rs`; Upper/lower and sidebar splitters: drag, persisted across restarts, double-click reset (UI test). Per-layout sizes and waveform-zone collapse / expand are not there; (SPEC §12; UX §2.6) | 2 |  |
| Status band: analysis / sync state, list summary (tracks, time, size) | DONE | `crates/od-ui/src/shell.rs`; Activity (analysis progress) at the left, the last message, and the open list summary (tracks, time, size, selection) at the right; (SPEC §12; UX §1.6) | 3 |  |
| UI scaling 0.75–2.0 without clipping | DONE | `crates/od-ui/src/app.rs::App::new`; Preferences > View > UI scale 75–200 % (applies after a restart); the view test checks the Prepare view at every scale on a 2560×1600 display for clipping and overlap; (SPEC §12, §16.4) | 3 |  |
| Colour-blind palette and high-contrast theme | DONE | Preferences > View switches (and the CLI flags) set the colour-blind-safe palette and the high-contrast theme live; stored in the settings; (SPEC §12) | 1 |  |
| English and German UI | DONE | `crates/od-i18n`; (SPEC §2) | 2 |  |
| Lite / Full performance profiles | DONE | (SPEC §3.2) | 2 |  |
| Raspberry Pi kiosk (linuxkms, Perform, controller browsing) | DONE (beyond the usual) | Built and driven headless on CI (`packaging/ci/headless-e2e.sh`); no cargo test; (SPEC §3.3) | — |  |
| Keyboard shortcuts for layouts and a View menu | DONE | `crates/od-ui/ui/app.slint::MenuBar`; View menu (modes, Prepare layout, info / related / My Tag / filter panels, sections) and keys ⌥⌘4 / ⌥⌘5 (layouts), ⌥⌘I / ⌥⌘E / ⌥⌘G (panels); the menu bar is native on macOS, elsewhere its actions are reached by the keys; (UX §1.9, §4 P3.7) | 3 |  |

## 13. DMX lighting

| Feature | Status | Evidence / note | Tests | Prio |
|---|---|---|---|---|
| DMX outputs: FLX10 DMX port, Enttec USB-DMX, Art-Net, sACN, several universes | DONE | Art-Net, sACN and USB-DMX (Enttec Pro) from the LIGHTING view or od-cli, several universes; the FLX10 built-in DMX port is tracked separately (lighting-flx10-dmx, M4); (SPEC §10.5) | 4 | P2 |
| DMX out through the DDJ-FLX10's built-in DMX port | MISSING, deferred to M4 | The controller's DMX port is driven by its vendor USB protocol, which needs the USB capture with the controller (M4, SPEC §9.3); every other output works today; (SPEC §10.5) | — | P3 |
| Fixture library (OFL), patch, groups, moving heads / bars / PARs / strobes / lasers / fog | DONE | Built-in PAR / mover / bar / strobe / blinder / laser / fog types and Open Fixture Library files (IMPORT FIXTURE FILE… or od-cli), patch with groups and overlap checks; (SPEC §10.5) | 6 | P2 |
| Automatic light show from beat, phrase, energy and the mix (crossfade-aware) | DONE | AUTO follows phrase, beat and energy; looks of both decks blend by their share of the mix; phrase changes crossfade over one bar; the LIGHTING view and the Perform strip show what it follows; (SPEC §10.5) | 5 | P2 |
| Easy manual mode: scene pads, dimmer, strobe, blackout, tap/sync, controller pad mode | DONE | EASY: look pads, colour swatches, master dimmer, BLACKOUT, FLASH / STROBE / FOG hold, TAP tempo when no deck plays; LIGHTING pad mode on the decks (and the controller pads): A–F looks, G strobe, H flash; (SPEC §10.5) | 4 | P2 |
| Pro mode: programmer, effects engine, cue stacks, per-track lighting cues, MIDI mapping | DONE | PRO: program / colour / attribute effect (sine, saw, square, random on dimmer, pan, tilt; phase spread; beat-synced) and an XY pan / tilt programmer per group; cue stack with GO / BACK / STORE and beat fades; track cue timeline (cues stored at bars of the track you hear, recalled whenever it plays); every lighting control MIDI-mappable (`lighting.*` mapping actions); (SPEC §10.5) | 6 | P2 |
| LIGHTING tab, split view in Perform, detachable window, 2D visualiser | DONE | LIGHTING tab (topbar, View menu, Alt+3) with RIG \| STAGE (live 2D visualiser) \| SHOW, a lighting strip inside Perform (LIGHT, Alt+K) and a detachable LIGHTING window for a second screen (WINDOW); (SPEC §10.5) | 4 | P2 |

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
| DONE | 201 |
| PARTIAL | 0 |
| ENGINE-ONLY | 0 |
| MISSING | 1 |
| deferred (in scope, later milestone) | 7 |
| OUT-OF-SCOPE | 5 |
| **Total** | **214** |

## P1 gaps and implementation notes

1. **Persist cue edits made in Perform.** Hot cues (incl. loop hot cues) and
   memory cues set or deleted on pads/controller only change engine state.
   *`od-ui` (`perform.rs`)*: after `HotCue`/`DeleteHotCue`/memory-cue
   actions, read the deck's cue table from `DeckState` (or mirror the
   command), build `Vec<Cue>` and call `Library::set_cues` with a journal
   label, unless the deck plays from a stick (`loaded_from_stick`). Wire the
   dropped `MemoryCueSet/MemoryCueDelete` actions the same way. Sync then
   follows automatically via the change log.
2. **Beat FX and Colour FX selection.** *Done 2026-11-06:* FX panel, CFX selector,
   keyboard and controller inputs with LEDs (`fx.rs`, `controller.rs`). Original note:
   engine is complete; nothing drives it.
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
7. **Controller coverage for mapped but dropped actions.** *Mostly done 2026-11-06*
   (FX, sampler, pad modes, key sync, tap BPM, memory cues, booth / master cue,
   stems); `deck.search` and `browse.related / view_toggle` remain. `handle_action`
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
tree navigation; phrase display (in progress); waveform zoom; EQ/fader/crossfader
curve settings (if not covered by P1-8); FLAC/MP3 recording and silence
auto start; batch stem separation in Prepare.

### P3 summary

Artwork display, duplicate check, XML/iTunes exchange, consolidate UI,
variable-tempo grid editing, BPM range setting, waveform style options,
vocal position, active loop, stem FX, Smart
CFX/Smart Fader, mic/aux input, auto-gain toggle, recording folder/level
settings, `exportExt.pdb`, `MYSETTING`, MIDI learn UI, FLX10 utility settings
UI and jog displays (M4).
