//! CDJ practice mode (SPEC §10.3c): player screens in Perform, the player's
//! cue rules, no on-screen sync, and a checklist that ticks itself off from
//! the engine state while the DJ works through the club workflow.

use std::rc::Rc;

use od_controller::PadMode;
use od_engine::{DeckCommand, EngineState, TempoRange};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::app::{App, handler};
use crate::{CdjItem, CdjStep, engine};
use od_core::TrackId;
use od_library::{Library, NodeId, NodeKind};

/// Seconds both decks must stay beat-aligned for the last step.
const ALIGNED_FOR: f64 = 4.0;
/// BPM difference that counts as matched.
const BPM_MATCH: f64 = 0.1;
/// Beat phase difference (fraction of a beat) that counts as aligned.
const PHASE_MATCH: f64 = 0.06;

pub const STEPS: [&str; 5] = [
    "Load tracks on two decks",
    "Set a cue point while paused",
    "Start a deck with PLAY",
    "Match the other deck's tempo by ear",
    "Start it on the one and keep the beats together",
];

/// Checklist progress (latched: a step stays done).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Checklist {
    pub done: [bool; STEPS.len()],
    aligned_since: Option<u64>,
}

impl Checklist {
    /// Updates from one engine state (pure).
    pub fn update(&mut self, s: &EngineState) {
        let n = s.decks_count.min(s.decks.len());
        let decks = &s.decks[..n];
        let loaded: Vec<usize> = (0..n).filter(|&i| decks[i].track.is_some()).collect();
        if loaded.len() >= 2 {
            self.done[0] = true;
        }
        if loaded
            .iter()
            .any(|&i| !decks[i].playing && decks[i].cue_point > 0.05)
        {
            self.done[1] = true;
        }
        if loaded.iter().any(|&i| decks[i].playing) {
            self.done[2] = true;
        }
        // A pair of loaded decks without sync, one playing, same tempo.
        let pair = loaded
            .iter()
            .flat_map(|&a| loaded.iter().map(move |&b| (a, b)))
            .find(|&(a, b)| {
                a < b
                    && !decks[a].sync
                    && !decks[b].sync
                    && (decks[a].playing || decks[b].playing)
                    && decks[a].bpm > 0.0
                    && (decks[a].bpm - decks[b].bpm).abs() < BPM_MATCH
            });
        if pair.is_some() {
            self.done[3] = true;
        }
        let aligned = pair.is_some_and(|(a, b)| {
            decks[a].playing && decks[b].playing && {
                let d = (decks[a].beat - decks[b].beat).rem_euclid(1.0);
                d.min(1.0 - d) < PHASE_MATCH
            }
        });
        if aligned {
            let since = *self.aligned_since.get_or_insert(s.frames);
            if (s.frames - since) as f64 / s.sample_rate.max(1) as f64 >= ALIGNED_FOR {
                self.done[4] = true;
            }
        } else {
            self.aligned_since = None;
        }
    }
}

#[derive(Default)]
pub struct CdjRuntime {
    pub checklist: Checklist,
    shown: Option<[bool; STEPS.len()]>,
    /// Beat of the bar per deck as last shown (beat-position indicator).
    beats_shown: Vec<i32>,
    /// Browse screen: deck (1-based) and the levels opened so far.
    browse: Option<(i32, Vec<Level>)>,
    /// What the rows of the current level open.
    targets: Vec<Target>,
}

/// One level of the browse screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Level {
    /// Sources: all tracks, the playlist tree.
    Root,
    /// Folders and lists under a parent (`None` = top level).
    Tree(Option<NodeId>),
    /// Tracks of a list (`None` = all tracks).
    Tracks(Option<NodeId>),
}

/// What a browse row does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Open(LevelRef),
    Load(TrackId),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LevelRef {
    Tree(Option<NodeId>),
    Tracks(Option<NodeId>),
}

/// Rows and their targets for one level (pure on the library).
pub fn browse_rows(lib: &Library, level: &Level) -> (String, Vec<(String, String, Target)>) {
    match level {
        Level::Root => (
            "SOURCE".into(),
            vec![
                (
                    "All tracks".into(),
                    String::new(),
                    Target::Open(LevelRef::Tracks(None)),
                ),
                (
                    "Playlists".into(),
                    String::new(),
                    Target::Open(LevelRef::Tree(None)),
                ),
            ],
        ),
        Level::Tree(parent) => {
            let nodes = lib.children(*parent).unwrap_or_default();
            let rows = nodes
                .into_iter()
                .map(|n| {
                    let target = if n.kind == NodeKind::Folder {
                        LevelRef::Tree(Some(n.id))
                    } else {
                        LevelRef::Tracks(Some(n.id))
                    };
                    (n.name, n.kind.as_str().to_uppercase(), Target::Open(target))
                })
                .collect();
            ("PLAYLISTS".into(), rows)
        }
        Level::Tracks(node) => {
            let tracks = match node {
                Some(id) => lib.node_tracks(*id).unwrap_or_default(),
                None => lib
                    .tracks(od_library::SortKey::Artist, false)
                    .unwrap_or_default(),
            };
            let rows = tracks
                .into_iter()
                .map(|t| {
                    let sub = format!(
                        "{} {}",
                        t.bpm.map(|b| format!("{b:.1}")).unwrap_or_default(),
                        t.key.map(|k| k.to_string()).unwrap_or_default()
                    );
                    (
                        format!("{} — {}", t.tags.artist, t.tags.title),
                        sub.trim().to_string(),
                        Target::Load(t.id),
                    )
                })
                .collect();
            ("TRACKS".into(), rows)
        }
    }
}

fn show_browse(app: &App) {
    let g = app.ui.global::<crate::Cdj>();
    let mut rt = app.cdj.borrow_mut();
    let Some((deck, levels)) = rt.browse.clone() else {
        g.set_browse_deck(0);
        return;
    };
    let level = levels.last().cloned().unwrap_or(Level::Root);
    let (title, rows) = {
        let st = app.state.borrow();
        browse_rows(st.src(), &level)
    };
    rt.targets = rows.iter().map(|r| r.2).collect();
    let items: Vec<CdjItem> = rows
        .into_iter()
        .map(|(t, s, target)| CdjItem {
            title: od_i18n::tr(&t).to_string().into(),
            sub: s.into(),
            folder: matches!(target, Target::Open(_)),
        })
        .collect();
    g.set_browse_title(od_i18n::tr(&title).to_string().into());
    g.set_browse_items(ModelRc::new(VecModel::from(items)));
    g.set_browse_deck(deck);
}

/// The next tempo range of the player's range button.
pub fn next_range(r: TempoRange) -> TempoRange {
    match r {
        TempoRange::Six => TempoRange::Ten,
        TempoRange::Ten => TempoRange::Sixteen,
        TempoRange::Sixteen => TempoRange::Wide,
        TempoRange::Wide => TempoRange::Six,
    }
}

pub(crate) fn set_enabled(app: &App, on: bool) {
    let g = app.ui.global::<crate::Cdj>();
    g.set_enabled(on);
    if on {
        engine::ensure_engine(app);
        let decks = engine::snapshot(app).map_or(2, |s| s.decks_count);
        for d in 0..decks {
            // Players have hot cues on their pads and no sync button.
            crate::decks::set_pad_mode(app, d, PadMode::HotCue);
            engine::deck_cmd(app, d, DeckCommand::SetSync(false));
        }
        g.set_show_steps(true);
    }
    app.status(if on {
        "CDJ practice mode: sync is off, beatmatch by ear"
    } else {
        "CDJ practice mode off"
    });
}

pub(crate) fn toggle(app: &App) {
    let on = app.ui.global::<crate::Cdj>().get_enabled();
    set_enabled(app, !on);
}

pub(crate) fn wire(app: &Rc<App>) {
    let g = app.ui.global::<crate::Cdj>();
    g.on_set_enabled(handler!(app, |a, on: bool| set_enabled(&a, on)));
    g.on_browse(handler!(app, |a, d: i32| {
        {
            let mut rt = a.cdj.borrow_mut();
            rt.browse = match rt.browse.take() {
                Some((open, _)) if open == d => None,
                _ => Some((d, vec![Level::Root])),
            };
        }
        show_browse(&a);
    }));
    g.on_browse_back(handler!(app, |a| {
        {
            let mut rt = a.cdj.borrow_mut();
            if let Some((_, levels)) = rt.browse.as_mut() {
                if levels.len() > 1 {
                    levels.pop();
                } else {
                    rt.browse = None;
                }
            }
        }
        show_browse(&a);
    }));
    g.on_browse_open(handler!(app, |a, i: i32| {
        let target = a.cdj.borrow().targets.get(i.max(0) as usize).copied();
        match target {
            Some(Target::Open(r)) => {
                if let Some((_, levels)) = a.cdj.borrow_mut().browse.as_mut() {
                    levels.push(match r {
                        LevelRef::Tree(p) => Level::Tree(p),
                        LevelRef::Tracks(n) => Level::Tracks(n),
                    });
                }
                show_browse(&a);
            }
            Some(Target::Load(id)) => {
                let deck = a.cdj.borrow().browse.as_ref().map(|b| b.0).unwrap_or(1);
                a.cdj.borrow_mut().browse = None;
                show_browse(&a);
                crate::decks::load_track(&a, (deck - 1).max(0) as usize, id, None);
            }
            None => {}
        }
    }));
    g.on_action(handler!(app, |a, d: i32, action: slint::SharedString| {
        crate::commands::run(&a, &format!("deck{d}.{action}"))
    }));
    g.on_cue(handler!(app, |a, d: i32, down: bool| deck(
        &a,
        d,
        DeckCommand::Cue(down)
    )));
    g.on_hot_cue(handler!(app, |a, d: i32, slot: i32, down: bool| deck(
        &a,
        d,
        DeckCommand::HotCue {
            slot: slot.clamp(0, 7) as usize,
            pressed: down
        }
    )));
    g.on_search(handler!(app, |a, d: i32, rate: f32| deck(
        &a,
        d,
        DeckCommand::Search(rate as f64)
    )));
    g.on_tempo(handler!(app, |a, d: i32, v: f32| a
        .ui
        .invoke_deck_tempo(d, v)));
    g.on_tempo_range(handler!(app, |a, d: i32| {
        let i = (d - 1).max(0) as usize;
        let range = engine::snapshot(&a)
            .map(|s| s.decks[i].tempo_range)
            .unwrap_or_default();
        deck(&a, d, DeckCommand::SetTempoRange(next_range(range)));
    }));
}

fn deck(app: &App, d: i32, cmd: DeckCommand) {
    if d < 1 || !engine::ensure_engine(app) {
        return;
    }
    engine::deck_cmd(app, d as usize - 1, cmd);
}

/// Beat of the bar at each deck's playhead (index deck - 1), as the
/// beat-position indicator shows it: 1..4, 0 for a deck without beatgrid.
pub(crate) fn beats_in_bar(state: &EngineState) -> Vec<i32> {
    state
        .decks
        .iter()
        .map(|d| {
            if d.track.is_some() {
                d.beat_in_bar as i32
            } else {
                0
            }
        })
        .collect()
}

/// Per engine frame while CDJ mode is on: advances the checklist and the
/// beat-position indicators.
pub(crate) fn frame(app: &App, state: &EngineState) {
    let g = app.ui.global::<crate::Cdj>();
    if !g.get_enabled() {
        return;
    }
    let mut rt = app.cdj.borrow_mut();
    // Beat-position indicator: the engine's beat of the bar (1..4, 0
    // without grid), pushed only when a deck's beat changes.
    let beats = beats_in_bar(state);
    if rt.beats_shown != beats {
        g.set_beat_in_bar(ModelRc::new(VecModel::from(beats.clone())));
        rt.beats_shown = beats;
    }
    rt.checklist.update(state);
    let done = rt.checklist.done;
    if rt.shown != Some(done) {
        rt.shown = Some(done);
        let steps: Vec<CdjStep> = STEPS
            .iter()
            .zip(done)
            .map(|(t, d)| CdjStep {
                text: od_i18n::tr(t).into(),
                done: d,
            })
            .collect();
        g.set_steps(ModelRc::new(VecModel::from(steps)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::TrackId;
    use od_engine::DeckState;

    fn loaded(id: i64) -> DeckState {
        DeckState {
            track: Some(TrackId(id)),
            bpm: 124.0,
            ..Default::default()
        }
    }

    #[test]
    fn checklist_follows_the_workflow() {
        let mut c = Checklist::default();
        let mut s = EngineState::default();
        s.decks[0] = loaded(1);
        c.update(&s);
        assert_eq!(c.done, [false; 5]);
        s.decks[1] = DeckState {
            bpm: 126.0,
            ..loaded(2)
        };
        s.decks[1].cue_point = 12.0;
        c.update(&s);
        assert_eq!(c.done[..2], [true, true]);
        s.decks[0].playing = true;
        c.update(&s);
        assert!(c.done[2] && !c.done[3], "tempos differ");
        s.decks[1].bpm = 124.05;
        c.update(&s);
        assert!(c.done[3]);
        // Both playing, in phase, for four seconds.
        s.decks[1].playing = true;
        s.decks[0].beat = 32.02;
        s.decks[1].beat = 8.0;
        for k in 0..5 {
            s.frames = k * 48_000;
            c.update(&s);
        }
        assert!(c.done[4]);
    }

    #[test]
    fn sync_does_not_count_and_drift_resets() {
        let mut c = Checklist::default();
        let mut s = EngineState::default();
        s.decks[0] = DeckState {
            playing: true,
            sync: true,
            ..loaded(1)
        };
        s.decks[1] = DeckState {
            playing: true,
            ..loaded(2)
        };
        c.update(&s);
        assert!(!c.done[3], "synced decks are not beatmatched by ear");
        s.decks[0].sync = false;
        s.decks[0].beat = 0.0;
        s.decks[1].beat = 0.0;
        s.frames = 0;
        c.update(&s);
        s.decks[1].beat = 0.3; // drifted
        s.frames = 3 * 48_000;
        c.update(&s);
        s.decks[1].beat = 0.0;
        s.frames = 6 * 48_000;
        c.update(&s);
        assert!(!c.done[4], "alignment restarted after the drift");
    }

    #[test]
    fn beat_position_follows_the_grid() {
        let mut s = EngineState::default();
        s.decks[0] = DeckState {
            beat_in_bar: 3,
            ..loaded(1)
        };
        // Deck 2 empty; a stale beat without a track shows nothing.
        s.decks[1].beat_in_bar = 2;
        let b = beats_in_bar(&s);
        assert_eq!(&b[..2], &[3, 0]);
    }

    #[test]
    fn range_button_cycles() {
        assert_eq!(next_range(TempoRange::Six), TempoRange::Ten);
        assert_eq!(next_range(TempoRange::Wide), TempoRange::Six);
    }
}
