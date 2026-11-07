//! The command core shared by all modules: status messages, error
//! reporting, the refresh after a library edit, and undo / redo.

use crate::app::App;
use crate::{browser, engine, player, shell};
use slint::Model;
use std::rc::Rc;

impl App {
    /// Shows a status message in the current language.
    pub(crate) fn status(&self, text: impl Into<String>) {
        self.ui
            .set_status_text(od_i18n::translate_message(&text.into()).into());
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
            self.after_edit(
                &label
                    .map(|l| format!("Undid: {}", od_i18n::translate_message(&l)))
                    .unwrap_or_else(|| "Nothing to undo".into()),
            );
        }
    }

    pub(crate) fn redo(&self) {
        let r = self.state.borrow_mut().lib.redo();
        if let Some(label) = self.report(r) {
            self.after_edit(
                &label
                    .map(|l| format!("Redid: {}", od_i18n::translate_message(&l)))
                    .unwrap_or_else(|| "Nothing to redo".into()),
            );
        }
    }
}

/// Runs a registered action (see [`crate::keymap`]) — the keyboard's way
/// into every module.
pub(crate) fn run(app: &Rc<App>, id: &str) {
    use slint::SharedString as S;
    let ui = &app.ui;
    let slot = |l: &str| {
        l.bytes()
            .next()
            .map(|b| (b.saturating_sub(b'a')) as i32)
            .unwrap_or(0)
            .clamp(0, 7)
    };
    match id {
        "prefs.open" => crate::preferences::open(app),
        "app.check-updates" => crate::update::check_now(app, true),
        "library.undo" => app.undo(),
        "library.redo" => app.redo(),
        "mode.prepare" => ui.invoke_set_mode(S::from("prepare")),
        "mode.perform" => ui.invoke_set_mode(S::from("perform")),
        "mode.lighting" => ui.invoke_set_mode(S::from("lighting")),
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
        "perform.fx.panel" => crate::fx::toggle_panel(app),
        "perform.cdj" => crate::cdj::toggle(app),
        "perform.layout.browse" => crate::decks::toggle_browse_layout(app),
        "perform.layout.vertical" => crate::decks::toggle_vertical_layout(app),
        "perform.light.panel" => {
            let g = slint::ComponentHandle::global::<crate::Lighting>(ui);
            g.set_panel(!g.get_panel());
        }
        "perform.sampler.panel" => crate::sampler::toggle_panel(app),
        // Source rail: the sampler lives in Perform; show its panel there.
        "browser.source.sampler" => {
            if ui.get_mode() != "perform" {
                ui.invoke_set_mode(S::from("perform"));
            }
            if !ui.get_sampler_panel() {
                crate::sampler::toggle_panel(app);
            }
        }
        "colorfx.next" => crate::fx::step_color_fx(app, 1),
        "colorfx.prev" => crate::fx::step_color_fx(app, -1),
        "sampler.bank.prev" => crate::sampler::step_bank_by(app, -1),
        "sampler.bank.next" => crate::sampler::step_bank_by(app, 1),
        "sampler.seq.play" => crate::sampler::toggle_seq_play(app),
        "sampler.stop.all" => {
            engine::ensure_engine(app);
            engine::send(
                app,
                od_engine::Command::Sampler(od_engine::SamplerCommand::StopAll),
            );
        }
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
            let row = app
                .state
                .borrow()
                .browser
                .selection
                .iter()
                .next_back()
                .copied();
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
        "browser.taglist.add" => ui.invoke_add_to_tag_list(),
        "browser.filter" => {
            if ui.get_section() != "library" {
                ui.invoke_select_section(S::from("library"));
            }
            ui.invoke_toggle_filter();
        }
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
        "player.grid.tap" => player::tap(app),
        "player.phrase.cut" => crate::phrase_edit::edit(app, crate::phrase_edit::Op::Cut),
        "player.phrase.boundary" => crate::phrase_edit::edit(app, crate::phrase_edit::Op::Boundary),
        "player.phrase.clear" => crate::phrase_edit::edit(app, crate::phrase_edit::Op::Clear),
        "player.edit.in" => crate::track_edit::mark(app, false),
        "player.edit.out" => crate::track_edit::mark(app, true),
        "player.edit.cut" => crate::track_edit::apply(app, crate::track_edit::EditOp::Cut),
        "player.edit.trim" => crate::track_edit::apply(app, crate::track_edit::EditOp::Trim),
        "player.edit.repeat" => crate::track_edit::apply(app, crate::track_edit::EditOp::Repeat),
        "view.layout.player" => shell::set_prepare_layout(app, "player"),
        "view.layout.browser" => shell::set_prepare_layout(app, "browser"),
        "view.layout.two" => shell::set_prepare_layout(app, "two"),
        "view.layout.dual" => shell::set_prepare_layout(app, "dual"),
        "view.layout.simple" => shell::set_prepare_layout(app, "simple"),
        "view.panel.info" => ui.set_info_open(!ui.get_info_open()),
        "view.panel.related" => {
            ui.set_related_open(!ui.get_related_open());
            ui.invoke_panels_changed();
        }
        "view.panel.my-tag" => {
            ui.set_my_tag_open(!ui.get_my_tag_open());
            ui.invoke_panels_changed();
        }
        "perform.mixer.panel" => shell::toggle_panel(app, "mixer"),
        "perform.mpl.panel" => shell::toggle_panel(app, "mpl"),
        "view.panel.rec" => shell::toggle_panel(app, "rec"),
        "view.panel.sub-browser" => browser::sub::toggle(app),
        "browser.duplicates" => browser::open_duplicates(app),
        "browser.stems" => crate::stems_batch::separate_selected(app),
        "library.export.xml"
        | "library.import.xml"
        | "library.export.archive"
        | "library.restore.archive"
        | "library.consolidate"
        | "library.import.rekordbox-settings"
        | "library.import.itunes" => crate::import::library_action(app, id),
        _ => {
            if let Some(l) = id.strip_prefix("perform.layout.set.") {
                crate::decks::set_perform_layout(app, l);
            } else if let Some(l) = id.strip_prefix("player.hotcue.clear.") {
                ui.invoke_remove_hot_cue(slot(l));
            } else if let Some(l) = id.strip_prefix("player.hotcue.") {
                ui.invoke_hot_cue(slot(l));
            } else if let Some(rest) = id.strip_prefix("browser.load.deck") {
                if let Ok(n) = rest.parse::<i32>() {
                    ui.invoke_deck_load(n);
                }
            } else if let Some(n) = id
                .strip_prefix("sampler.slot")
                .and_then(|n| n.parse::<usize>().ok())
            {
                engine::ensure_engine(app);
                engine::send(
                    app,
                    od_engine::Command::Sampler(od_engine::SamplerCommand::Trigger { slot: n - 1 }),
                );
            } else if let Some((unit, action)) =
                id.strip_prefix("fx").and_then(|r| r.split_once('.'))
            {
                if let Ok(n) = unit.parse::<usize>() {
                    fx_action(app, id, n.saturating_sub(1), action);
                }
            } else if let Some((deck, action)) =
                id.strip_prefix("deck").and_then(|r| r.split_once('.'))
            {
                if let Ok(n) = deck.parse::<usize>() {
                    deck_action(app, n, action);
                }
            } else {
                tracing::warn!(id, "unknown action");
            }
        }
    }
}

/// Marks a held keyboard action that is not a deck's (Release FX).
const NO_DECK: usize = usize::MAX;

/// Beat FX unit actions (0-based unit).
fn fx_action(app: &App, id: &str, u: usize, action: &str) {
    use crate::fx;
    use od_engine::ReleaseFxKind as R;
    let release = |kind| {
        app.held.borrow_mut().push((id.to_string(), NO_DECK));
        fx::release(app, u, kind, true);
    };
    match action {
        "on" => fx::toggle_on(app, u),
        "next" => fx::step_unit_kind(app, u, 1),
        "beat.down" => fx::step_unit_beats(app, u, -1),
        "beat.up" => fx::step_unit_beats(app, u, 1),
        "tap" => fx::tap(app, u),
        "release.brake" => release(R::VinylBrake),
        "release.backspin" => release(R::Backspin),
        "release.echo" => release(R::EchoOut),
        a => tracing::warn!(action = a, "unknown FX action"),
    }
}

/// Ends a held Release FX started by the keyboard action `id`.
fn end_fx_release(app: &App, id: &str) {
    use od_engine::ReleaseFxKind as R;
    let Some((unit, action)) = id.strip_prefix("fx").and_then(|r| r.split_once('.')) else {
        return;
    };
    let u = unit.parse::<usize>().unwrap_or(1).saturating_sub(1);
    let kind = match action {
        "release.brake" => R::VinylBrake,
        "release.backspin" => R::Backspin,
        _ => R::EchoOut,
    };
    crate::fx::release(app, u, kind, false);
}

/// Deck actions (1-based deck number).
fn deck_action(app: &App, n: usize, action: &str) {
    use od_engine::{DeckCommand as D, MemoryCueJump};
    let ui = &app.ui;
    let d = n as i32;
    let i = n - 1;
    let slot = |l: &str| {
        l.bytes()
            .next()
            .map(|b| (b.saturating_sub(b'a')) as usize)
            .unwrap_or(0)
            .min(7)
    };
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
        "loop.1" | "loop.2" | "loop.4" | "loop.8" => {
            ui.invoke_deck_loop(d, action[5..].parse().unwrap_or(4.0))
        }
        _ => {
            engine::ensure_engine(app);
            let cmd = match action {
                "start" => Some(D::Seek { seconds: 0.0 }),
                "bend.up" | "bend.down" => {
                    app.held.borrow_mut().push((action.to_string(), i));
                    Some(D::SetPitchBend(if action == "bend.up" {
                        0.04
                    } else {
                        -0.04
                    }))
                }
                "tempo.up" | "tempo.down" => {
                    let v = state().map(|s| s.tempo_fader).unwrap_or(0.0);
                    Some(D::SetTempoFader(
                        (v + if action == "tempo.up" { 0.01 } else { -0.01 }).clamp(-1.0, 1.0),
                    ))
                }
                "jump.fwd" => Some(D::BeatJumpStep { forward: true }),
                "jump.back" => Some(D::BeatJumpStep { forward: false }),
                "loop.in" => Some(D::LoopIn),
                "loop.out" => Some(D::LoopOut),
                "loop.exit" => Some(if state().is_some_and(|s| s.loop_state.active) {
                    D::LoopExit
                } else {
                    D::Reloop
                }),
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
                "padmode.next" | "padmode.prev" => {
                    let mode = crate::pads::step_mode(
                        app.decks.borrow().pad_mode[i],
                        if action == "padmode.next" { 1 } else { -1 },
                    );
                    crate::decks::set_pad_mode(app, i, mode);
                    None
                }
                "hotcue.bank" => {
                    let bank = state().map(|s| s.hot_cue_bank).unwrap_or(0);
                    Some(D::SetHotCueBank(crate::pads::step_hot_cue_bank(bank, 1)))
                }
                "keysync" => {
                    crate::decks::toggle_key_sync(app, i);
                    None
                }
                "keyshift.up" | "keyshift.down" => {
                    crate::decks::key_shift(app, i, if action == "keyshift.up" { 1 } else { -1 });
                    None
                }
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
    let Some(chord) = crate::keymap::Chord::from_event(
        text,
        false,
        false,
        false,
        false,
        crate::keymap::Layout::detect(),
    ) else {
        return;
    };
    let held: Vec<(String, usize)> = std::mem::take(&mut *app.held.borrow_mut());
    let keymap_hits: Vec<String> = {
        let st = app.state.borrow();
        held.iter()
            .filter(|(a, deck)| {
                st.keymap
                    .bindings(&held_id(a, *deck))
                    .iter()
                    .any(|c| c.key == chord.key)
            })
            .map(|(a, d)| format!("{d}{a}"))
            .collect()
    };
    let mut keep = Vec::new();
    for (a, deck) in held {
        if keymap_hits.contains(&format!("{deck}{a}")) {
            if deck == NO_DECK {
                end_fx_release(app, &a);
            } else {
                engine::deck_cmd(app, deck, od_engine::DeckCommand::SetPitchBend(0.0));
            }
        } else {
            keep.push((a, deck));
        }
    }
    *app.held.borrow_mut() = keep;
}

/// The action id of a held entry.
fn held_id(action: &str, deck: usize) -> String {
    if deck == NO_DECK {
        action.to_string()
    } else {
        format!("deck{}.{action}", deck + 1)
    }
}
