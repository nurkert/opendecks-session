//! Mix Point Link: a mix-out point on the outgoing track and a mix-in point
//! on the incoming one (stored per track in the library meta, so they come
//! back with the track). With LINK on, the deck that is not playing starts
//! from its mix-in point the moment the playing deck passes its mix-out
//! point. The panel sits above the waveforms (top bar MIX POINT).

use crate::app::App;
use crate::models::model;
use crate::{MplDeck, engine, format};
use od_core::{CueKind, TrackId};
use od_engine::DeckCommand;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// The two decks the panel shows (0-based).
const PANEL_DECKS: usize = 2;

/// Mix points of one track, seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct MixPoints {
    pub mix_in: Option<f64>,
    pub mix_out: Option<f64>,
}

fn meta_key(track: TrackId) -> String {
    format!("mpl.{}", track.0)
}

/// The stored mix points of `track`.
pub(crate) fn load(lib: &od_library::Library, track: TrackId) -> MixPoints {
    lib.meta(&meta_key(track))
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default()
}

fn store(lib: &mut od_library::Library, track: TrackId, p: MixPoints) {
    if let Err(e) = lib.set_meta(
        &meta_key(track),
        &serde_json::to_string(&p).unwrap_or_default(),
    ) {
        tracing::warn!(error = %e, "mix points not saved");
    }
}

/// The outgoing deck crossed its mix-out point between two frames (pure).
pub(crate) fn crossed(prev: f64, now: f64, mix_out: f64) -> bool {
    prev < mix_out && now >= mix_out && now - prev < 1.0
}

/// The point after (or before) `pos` among `points`, ignoring points within
/// 50 ms of it (pure).
pub(crate) fn step_point(points: &[f64], pos: f64, forward: bool) -> Option<f64> {
    let far = points.iter().copied().filter(|t| (t - pos).abs() > 0.05);
    if forward {
        far.filter(|t| *t > pos).reduce(f64::min)
    } else {
        far.filter(|t| *t < pos).reduce(f64::max)
    }
}

#[derive(Default)]
struct MplState {
    link: bool,
    /// Edited deck (0-based).
    edit: usize,
    /// Positions at the last frame, per panel deck.
    last: [Option<(Option<TrackId>, f64)>; PANEL_DECKS],
    shown: Vec<MplDeck>,
}

thread_local! {
    static STATE: RefCell<MplState> = RefCell::new(MplState::default());
    static TIMER: slint::Timer = slint::Timer::default();
}

/// Memory cues plus mix points of `track` (sorted).
fn points(app: &App, track: TrackId) -> (Vec<f64>, MixPoints) {
    let st = app.state.borrow();
    let mp = load(&st.lib, track);
    let mut v: Vec<f64> = st
        .lib
        .cues(track)
        .unwrap_or_default()
        .iter()
        .filter(|c| c.kind == CueKind::Memory)
        .map(|c| c.start)
        .collect();
    v.extend(mp.mix_in);
    v.extend(mp.mix_out);
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    (v, mp)
}

/// A panel button: "mix-in", "mix-out", "prev", "next", "edit" or "link".
pub(crate) fn act(app: &App, what: &str) {
    let edit = STATE.with(|s| s.borrow().edit);
    match what {
        "edit" => STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.edit = (s.edit + 1) % PANEL_DECKS;
        }),
        "link" => {
            let on = STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.link = !s.link;
                s.link
            });
            app.status(if on {
                "Mix Point Link on"
            } else {
                "Mix Point Link off"
            });
        }
        _ => {
            let Some(d) = engine::snapshot(app).map(|s| s.decks[edit].clone()) else {
                app.status("Load a track on the deck first");
                return;
            };
            let Some(track) = d.track else {
                app.status("Load a track on the deck first");
                return;
            };
            match what {
                "mix-in" | "mix-out" => {
                    let mut st = app.state.borrow_mut();
                    let mut p = load(&st.lib, track);
                    if what == "mix-in" {
                        p.mix_in = Some(d.position);
                    } else {
                        p.mix_out = Some(d.position);
                    }
                    store(&mut st.lib, track, p);
                }
                "prev" | "next" => {
                    let (pts, _) = points(app, track);
                    if let Some(t) = step_point(&pts, d.position, what == "next") {
                        engine::deck_cmd(app, edit, DeckCommand::Seek { seconds: t });
                    }
                }
                _ => {}
            }
        }
    }
    refresh(app);
}

/// The panel view model.
pub(crate) fn refresh(app: &App) {
    let snap = engine::snapshot(app);
    let (link, edit) = STATE.with(|s| (s.borrow().link, s.borrow().edit));
    let decks: Vec<MplDeck> = (0..PANEL_DECKS)
        .map(|i| {
            let track = snap.as_ref().and_then(|s| s.decks[i].track);
            let (count, mp) = track
                .map(|t| {
                    let (pts, mp) = points(app, t);
                    (pts.len(), mp)
                })
                .unwrap_or_default();
            let time = |t: Option<f64>| t.map(format::cue_time).unwrap_or_default();
            MplDeck {
                deck: i as i32 + 1,
                points: count as i32,
                mix_in: SharedString::from(time(mp.mix_in)),
                mix_out: SharedString::from(time(mp.mix_out)),
            }
        })
        .collect();
    let g = app.ui.global::<crate::Mpl>();
    g.set_link(link);
    g.set_edit_deck(edit as i32 + 1);
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let c = s.shown != decks;
        s.shown = decks.clone();
        c
    });
    if changed {
        g.set_decks(model(decks));
    }
}

/// Frame check: with LINK on, start the waiting deck at its mix-in point
/// when the playing deck passes its mix-out point.
fn tick(app: &App) {
    let Some(snap) = engine::snapshot(app) else {
        return;
    };
    let link = STATE.with(|s| s.borrow().link);
    for out in 0..PANEL_DECKS {
        let d = &snap.decks[out];
        let prev = STATE.with(|s| {
            std::mem::replace(&mut s.borrow_mut().last[out], Some((d.track, d.position)))
        });
        let (Some(track), true) = (d.track, d.playing) else {
            continue;
        };
        let Some((prev_track, prev_pos)) = prev else {
            continue;
        };
        if !link || prev_track != Some(track) {
            continue;
        }
        let Some(mix_out) = load(&app.state.borrow().lib, track).mix_out else {
            continue;
        };
        if !crossed(prev_pos, d.position, mix_out) {
            continue;
        }
        let inc = (out + 1) % PANEL_DECKS;
        let other = &snap.decks[inc];
        let (Some(next), false) = (other.track, other.playing) else {
            continue;
        };
        let start = load(&app.state.borrow().lib, next).mix_in.unwrap_or(0.0);
        engine::deck_cmd(app, inc, DeckCommand::Seek { seconds: start });
        engine::deck_cmd(app, inc, DeckCommand::Play);
        app.status(format!("Mix Point Link: deck {} comes in", inc + 1));
    }
    if app.ui.get_mpl_panel() && app.ui.get_mode() == "perform" {
        refresh(app);
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    let weak = Rc::downgrade(app);
    app.ui.global::<crate::Mpl>().on_act(move |w| {
        if let Some(a) = weak.upgrade() {
            act(&a, w.as_str());
        }
    });
    let weak = Rc::downgrade(app);
    TIMER.with(|t| {
        t.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(40),
            move || {
                if let Some(a) = weak.upgrade() {
                    tick(&a);
                }
            },
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_the_mix_out_point() {
        assert!(crossed(59.98, 60.01, 60.0));
        assert!(!crossed(60.01, 60.05, 60.0), "already past");
        assert!(!crossed(59.0, 59.5, 60.0), "not there yet");
        assert!(
            !crossed(10.0, 61.0, 60.0),
            "a seek over the point does not fire"
        );
    }

    #[test]
    fn stepping_between_points() {
        let p = [10.0, 32.0, 64.0];
        assert_eq!(step_point(&p, 20.0, true), Some(32.0));
        assert_eq!(step_point(&p, 20.0, false), Some(10.0));
        assert_eq!(
            step_point(&p, 32.01, true),
            Some(64.0),
            "the point under the playhead is skipped"
        );
        assert_eq!(step_point(&p, 70.0, true), None);
    }

    #[test]
    fn mix_points_round_trip_in_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = od_library::Library::open(&dir.path().join("l.sqlite")).unwrap();
        let t = TrackId(7);
        assert_eq!(load(&lib, t), MixPoints::default());
        store(
            &mut lib,
            t,
            MixPoints {
                mix_in: Some(12.5),
                mix_out: None,
            },
        );
        assert_eq!(load(&lib, t).mix_in, Some(12.5));
    }
}
