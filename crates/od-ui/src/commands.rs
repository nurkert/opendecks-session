//! The command core shared by all modules: status messages, error
//! reporting, the refresh after a library edit, and undo / redo.

use crate::app::App;
use crate::{browser, engine, player, shell};
use slint::Model;
use std::rc::Rc;

impl App {
    /// Shows a status message in the current language.
    pub(crate) fn status(&self, text: impl Into<String>) {
        self.ui.set_status_text(od_i18n::translate_message(&text.into()).into());
    }

    /// Shows a library error and turns the result into an option.
    pub(crate) fn report<T>(&self, r: od_library::Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.status(format!("{e}"));
                None
            }
        }
    }

    /// Reloads the current list and every view that shows library data.
    pub(crate) fn refresh_library(&self) {
        crate::explorer::refresh_library_data(self);
        crate::decks::refresh_analysis(self);
        self.state.borrow_mut().load_rows();
        browser::refresh_tree(self);
        browser::refresh_tracks(self);
        player::refresh_detail(self);
        shell::refresh_status(self);
    }

    /// After a library edit: refresh and show `label` (if any).
    pub(crate) fn after_edit(&self, label: &str) {
        self.refresh_library();
        if !label.is_empty() {
            self.status(label.to_string());
        }
    }

    /// Tells the user that stick tracks cannot be edited; returns true when
    /// the current view is read-only.
    pub(crate) fn guard_read_only(&self) -> bool {
        if self.state.borrow().read_only() {
            self.status("Tracks on a stick are read-only here — edit them in your library and the stick follows");
            true
        } else {
            false
        }
    }

    pub(crate) fn undo(&self) {
        let r = self.state.borrow_mut().lib.undo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Undid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to undo".into()));
        }
    }

    pub(crate) fn redo(&self) {
        let r = self.state.borrow_mut().lib.redo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Redid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to redo".into()));
        }
    }
}

/// Runs a registered action (see [`crate::keymap`]) — the keyboard's way
/// into every module.
pub(crate) fn run(app: &Rc<App>, id: &str) {
    use slint::SharedString as S;
    let ui = &app.ui;
    let slot = |l: &str| l.bytes().next().map(|b| (b.saturating_sub(b'a')) as i32).unwrap_or(0).clamp(0, 7);
    match id {
        "prefs.open" => crate::preferences::open(app),
        "library.undo" => app.undo(),
        "library.redo" => app.redo(),
        "mode.prepare" => ui.invoke_set_mode(S::from("prepare")),
        "mode.perform" => ui.invoke_set_mode(S::from("perform")),
        "section.library" => ui.invoke_select_section(S::from("library")),
        "section.devices" => ui.invoke_select_section(S::from("devices")),
        "section.recordings" => ui.invoke_select_section(S::from("recordings")),
        "record.toggle" => ui.invoke_toggle_recording(),
        "devices.sync" => {
            let sel = ui.get_device_selected();
            if sel >= 0 {
                ui.invoke_sync_device(sel);
            } else {
                let ids: Vec<i32> = ui.get_devices().iter().map(|d| d.id).collect();
                ids.into_iter().for_each(|d| ui.invoke_sync_device(d));
            }
        }
        "devices.eject" => {
            let sel = ui.get_device_selected();
            if sel >= 0 {
                ui.invoke_eject_device(sel);
            }
        }
        "perform.quantize" => ui.invoke_deck_quantize(1),
        "browser.search" => {
            if ui.get_section() != "library" {
                ui.invoke_select_section(S::from("library"));
            }
            ui.set_search_focus(ui.get_search_focus() + 1);
        }
        "browser.up" => browser::step_selection(app, true),
        "browser.down" => browser::step_selection(app, false),
        "browser.list.prev" => browser::cycle_list(app, false),
        "browser.list.next" => browser::cycle_list(app, true),
        "browser.select.all" => browser::select_all(app),
        "browser.load" => {
            let row = app.state.borrow().browser.selection.iter().next_back().copied();
            if let Some(i) = row {
                ui.invoke_row_activated(i as i32);
            }
        }
        "browser.remove" => browser::remove_selected(app),
        "browser.new.playlist" => ui.invoke_create_node(S::from("playlist")),
        "browser.new.set" => ui.invoke_create_node(S::from("set")),
        "browser.new.folder" => ui.invoke_create_node(S::from("folder")),
        "browser.new.smart" => ui.invoke_create_node(S::from("smart")),
        "browser.delete.list" => ui.invoke_delete_node(),
        "browser.add.music" => ui.invoke_import_folder(),
        "browser.import.rekordbox" => ui.invoke_import_rekordbox(),
        "browser.find.moved" => ui.invoke_find_moved_files(),
        "player.preview" => player::toggle_preview(app),
        "player.cue" => ui.invoke_jump_start(),
        "player.memory.add" => ui.invoke_add_memory_cue(),
        "player.memory.delete" => player::delete_memory_cue(app),
        "player.cue.prev" => player::step_cue(app, false),
        "player.cue.next" => player::step_cue(app, true),
        "player.nudge.back" => ui.invoke_nudge(-0.05),
        "player.nudge.fwd" => ui.invoke_nudge(0.05),
        "player.nudge.back.coarse" => ui.invoke_nudge(-2.0),
        "player.nudge.fwd.coarse" => ui.invoke_nudge(2.0),
        "player.grid.earlier" => ui.invoke_grid_shift(-10),
        "player.grid.later" => ui.invoke_grid_shift(10),
        "player.grid.double" => ui.invoke_grid_scale(2.0),
        "player.grid.half" => ui.invoke_grid_scale(0.5),
        "player.grid.downbeat" => ui.invoke_grid_downbeat(),
        "player.grid.lock" => ui.invoke_grid_lock(),
        "player.analyse" => ui.invoke_analyse_selected(),
        _ => {
            if let Some(l) = id.strip_prefix("player.hotcue.clear.") {
                ui.invoke_remove_hot_cue(slot(l));
            } else if let Some(l) = id.strip_prefix("player.hotcue.") {
                ui.invoke_hot_cue(slot(l));
            } else if let Some(rest) = id.strip_prefix("browser.load.deck") {
                if let Ok(n) = rest.parse::<i32>() {
                    ui.invoke_deck_load(n);
                }
            } else if let Some(n) = id.strip_prefix("sampler.slot").and_then(|n| n.parse::<usize>().ok()) {
                engine::ensure_engine(app);
                engine::send(app, od_engine::Command::Sampler(od_engine::SamplerCommand::Trigger { slot: n - 1 }));
            } else if let Some((deck, action)) = id.strip_prefix("deck").and_then(|r| r.split_once('.')) {
                if let Ok(n) = deck.parse::<usize>() {
                    deck_action(app, n, action);
                }
            } else {
                tracing::warn!(id, "unknown action");
            }
        }
    }
}

/// Deck actions (1-based deck number).
fn deck_action(app: &App, n: usize, action: &str) {
    use od_engine::{DeckCommand as D, MemoryCueJump};
    let ui = &app.ui;
    let d = n as i32;
    let i = n - 1;
    let slot = |l: &str| l.bytes().next().map(|b| (b.saturating_sub(b'a')) as usize).unwrap_or(0).min(7);
    let state = || engine::snapshot(app).map(|s| s.decks[i]);
    match action {
        "play" => ui.invoke_deck_toggle_play(d),
        "cue" => ui.invoke_deck_cue(d),
        "sync" => ui.invoke_deck_sync(d),
        "master" => ui.invoke_deck_master(d),
        "keylock" => ui.invoke_deck_keylock(d),
        "loop.half" => ui.invoke_deck_loop_scale(d, 0.5),
        "loop.double" => ui.invoke_deck_loop_scale(d, 2.0),
        "loop.1_2" => ui.invoke_deck_loop(d, 0.5),
        "loop.1" | "loop.2" | "loop.4" | "loop.8" => ui.invoke_deck_loop(d, action[5..].parse().unwrap_or(4.0)),
        _ => {
            engine::ensure_engine(app);
            let cmd = match action {
                "start" => Some(D::Seek { seconds: 0.0 }),
                "bend.up" | "bend.down" => {
                    app.held.borrow_mut().push((action.to_string(), i));
                    Some(D::SetPitchBend(if action == "bend.up" { 0.04 } else { -0.04 }))
                }
                "tempo.up" | "tempo.down" => {
                    let v = state().map(|s| s.tempo_fader).unwrap_or(0.0);
                    Some(D::SetTempoFader((v + if action == "tempo.up" { 0.01 } else { -0.01 }).clamp(-1.0, 1.0)))
                }
                "jump.fwd" => Some(D::BeatJumpStep { forward: true }),
                "jump.back" => Some(D::BeatJumpStep { forward: false }),
                "loop.in" => Some(D::LoopIn),
                "loop.out" => Some(D::LoopOut),
                "loop.exit" => Some(if state().is_some_and(|s| s.loop_state.active) { D::LoopExit } else { D::Reloop }),
                "loop.auto" => {
                    if state().is_some_and(|s| s.loop_state.active) {
                        Some(D::LoopExit)
                    } else {
                        ui.invoke_deck_loop(d, 0.0);
                        None
                    }
                }
                "memory.next" => Some(D::MemoryCue(MemoryCueJump::Next)),
                "memory.prev" => Some(D::MemoryCue(MemoryCueJump::Previous)),
                a => {
                    if let Some(l) = a.strip_prefix("hotcue.clear.") {
                        Some(D::DeleteHotCue { slot: slot(l) })
                    } else if let Some(l) = a.strip_prefix("hotcue.") {
                        ui.invoke_deck_hot_cue(d, slot(l) as i32);
                        None
                    } else {
                        tracing::warn!(action = a, "unknown deck action");
                        None
                    }
                }
            };
            if let Some(c) = cmd {
                engine::deck_cmd(app, i, c);
            }
        }
    }
}

/// Key release: ends held actions (pitch bend) started by that key.
pub(crate) fn release(app: &App, text: &str) {
    let Some(chord) = crate::keymap::Chord::from_event(text, false, false, false, false, crate::keymap::Layout::detect()) else { return };
    let held: Vec<(String, usize)> = std::mem::take(&mut *app.held.borrow_mut());
    let keymap_hits: Vec<String> = {
        let st = app.state.borrow();
        held.iter()
            .filter(|(a, deck)| st.keymap.bindings(&format!("deck{}.{a}", deck + 1)).iter().any(|c| c.key == chord.key))
            .map(|(a, d)| format!("{d}{a}"))
            .collect()
    };
    let mut keep = Vec::new();
    for (a, deck) in held {
        if keymap_hits.contains(&format!("{deck}{a}")) {
            engine::deck_cmd(app, deck, od_engine::DeckCommand::SetPitchBend(0.0));
        } else {
            keep.push((a, deck));
        }
    }
    *app.held.borrow_mut() = keep;
}
