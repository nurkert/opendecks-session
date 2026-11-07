//! Phrase editing in the Prepare player. Directly on the phrase strip under
//! the overview: a click on a phrase opens a type picker beside it (and
//! MERGE WITH NEXT), its edges drag to a new bar; at the cursor (PHRASE tab
//! and keys): the kind, a split, a merge with the next phrase, the nearest
//! boundary moved to the cursor, and clearing all phrases. Every edit snaps
//! to the bar grid and is stored through the library (one undo step,
//! normalized with [`od_core::normalize_phrases`]); it reaches registered
//! sticks like any other edit.

use crate::app::{App, handler};
use od_core::{Beatgrid, Phrase, PhraseKind, normalize_phrases};
use slint::ComponentHandle;
use std::rc::Rc;

/// The kinds offered in the PHRASE tab, in button order.
pub(crate) const KINDS: [PhraseKind; 7] = [
    PhraseKind::Intro,
    PhraseKind::Verse,
    PhraseKind::Chorus,
    PhraseKind::Bridge,
    PhraseKind::Up,
    PhraseKind::Down,
    PhraseKind::Outro,
];

/// The beat a phrase edit at `t` seconds applies to: the nearest downbeat
/// within two beats (phrases start on bars), else the nearest beat. `None`
/// without a grid (pure).
pub(crate) fn edit_beat(grid: &Beatgrid, t: f64) -> Option<u32> {
    let n = grid.beats.len();
    if n == 0 {
        return None;
    }
    let i = grid
        .beat_position(t)
        .map(|b| b.round().clamp(0.0, (n - 1) as f64) as usize)
        .unwrap_or(0);
    let down = (i.saturating_sub(2)..=(i + 2).min(n - 1))
        .filter(|&j| grid.beats[j].bar_position == 1)
        .min_by(|a, b| {
            (grid.beats[*a].time - t)
                .abs()
                .total_cmp(&(grid.beats[*b].time - t).abs())
        });
    Some(down.unwrap_or(i) as u32)
}

/// Index of the phrase that holds `beat`.
pub(crate) fn phrase_at(phrases: &[Phrase], beat: u32) -> Option<usize> {
    phrases
        .iter()
        .position(|p| p.start_beat <= beat && beat < p.end_beat)
}

/// The phrase at `beat` takes `kind`; in a gap (or without phrases) a new
/// phrase of `kind` fills the gap around `beat` up to `total` beats (pure,
/// not normalized).
pub(crate) fn set_kind(phrases: &[Phrase], beat: u32, kind: PhraseKind, total: u32) -> Vec<Phrase> {
    let mut v = sorted(phrases);
    match phrase_at(&v, beat) {
        Some(i) => v[i].kind = kind,
        None => {
            let start = v
                .iter()
                .filter(|p| p.end_beat <= beat)
                .map(|p| p.end_beat)
                .max()
                .unwrap_or(0);
            let end = v
                .iter()
                .filter(|p| p.start_beat > beat)
                .map(|p| p.start_beat)
                .min()
                .unwrap_or(total);
            if end > start {
                v.push(Phrase {
                    start_beat: start,
                    end_beat: end,
                    kind,
                });
                v.sort_by_key(|p| p.start_beat);
            }
        }
    }
    v
}

/// The shortest part a cut may leave (beats: 4 bars; shorter fragments
/// would join a neighbour when normalized).
pub(crate) const MIN_PART: u32 = 16;

/// Splits the phrase at `beat` there; the part from `beat` on takes the
/// next kind that differs from the phrase and from the one after it (so
/// the cut survives normalization). `None` when no phrase holds `beat` or
/// either part would be shorter than [`MIN_PART`] (pure).
pub(crate) fn cut(phrases: &[Phrase], beat: u32) -> Option<Vec<Phrase>> {
    let mut v = sorted(phrases);
    let i = phrase_at(&v, beat)
        .filter(|&i| beat >= v[i].start_beat + MIN_PART && beat + MIN_PART <= v[i].end_beat)?;
    let kind = v[i].kind;
    let following = v
        .get(i + 1)
        .filter(|n| n.start_beat == v[i].end_beat)
        .map(|n| n.kind);
    let at = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
    let next = (1..KINDS.len())
        .map(|d| KINDS[(at + d) % KINDS.len()])
        .find(|k| Some(*k) != following)
        .unwrap_or(kind);
    let tail = Phrase {
        start_beat: beat,
        end_beat: v[i].end_beat,
        kind: next,
    };
    v[i].end_beat = beat;
    v.insert(i + 1, tail);
    Some(v)
}

/// Moves the boundary nearest to `beat` (a phrase start or end; a shared
/// boundary moves for both phrases) to `beat`. `None` when there is no
/// boundary or the move would pass the neighbouring boundary (pure).
pub(crate) fn move_boundary(phrases: &[Phrase], beat: u32) -> Option<Vec<Phrase>> {
    let v = sorted(phrases);
    let mut bounds: Vec<u32> = v.iter().flat_map(|p| [p.start_beat, p.end_beat]).collect();
    bounds.sort_unstable();
    bounds.dedup();
    let b = *bounds.iter().min_by_key(|b| b.abs_diff(beat))?;
    if b == beat {
        return None;
    }
    let out: Vec<Phrase> = v
        .iter()
        .map(|p| Phrase {
            start_beat: if p.start_beat == b {
                beat
            } else {
                p.start_beat
            },
            end_beat: if p.end_beat == b { beat } else { p.end_beat },
            kind: p.kind,
        })
        .collect();
    // Every phrase keeps its length and order.
    let ordered = out.iter().all(|p| p.end_beat > p.start_beat)
        && out.windows(2).all(|w| w[0].end_beat <= w[1].start_beat);
    ordered.then_some(out)
}

/// Moves the boundary nearest to `from` (a beat on or near an edge) to
/// `to`; `None` when there is none or the move would pass a neighbouring
/// boundary (pure).
pub(crate) fn move_edge(phrases: &[Phrase], from: u32, to: u32) -> Option<Vec<Phrase>> {
    let v = sorted(phrases);
    let mut bounds: Vec<u32> = v.iter().flat_map(|p| [p.start_beat, p.end_beat]).collect();
    bounds.sort_unstable();
    bounds.dedup();
    let b = *bounds.iter().min_by_key(|b| b.abs_diff(from))?;
    if b == to {
        return None;
    }
    let out: Vec<Phrase> = v
        .iter()
        .map(|p| Phrase {
            start_beat: if p.start_beat == b { to } else { p.start_beat },
            end_beat: if p.end_beat == b { to } else { p.end_beat },
            kind: p.kind,
        })
        .collect();
    let ordered = out.iter().all(|p| p.end_beat > p.start_beat)
        && out.windows(2).all(|w| w[0].end_beat <= w[1].start_beat);
    ordered.then_some(out)
}

/// The phrase at `beat` and the one right after it become one phrase of
/// the first one's kind; `None` without a following neighbour (pure).
pub(crate) fn merge(phrases: &[Phrase], beat: u32) -> Option<Vec<Phrase>> {
    let mut v = sorted(phrases);
    let i = phrase_at(&v, beat)?;
    let next = v.get(i + 1).filter(|n| n.start_beat == v[i].end_beat)?.end_beat;
    v[i].end_beat = next;
    v.remove(i + 1);
    Some(v)
}

fn sorted(phrases: &[Phrase]) -> Vec<Phrase> {
    let mut v = phrases.to_vec();
    v.sort_by_key(|p| p.start_beat);
    v
}

/// One phrase edit: at the cursor, or at a time (seconds) picked on the
/// phrase strip.
pub(crate) enum Op {
    Kind(PhraseKind),
    Cut,
    Boundary,
    Clear,
    Merge,
    /// The phrase at that time takes the kind.
    KindAt(f64, PhraseKind),
    /// The phrase at that time joins the next one.
    MergeAt(f64),
    /// The boundary at the first time moves to the second (bar-snapped).
    MoveEdge(f64, f64),
}

/// Applies `op` to the current track's phrases at the cursor and stores the
/// normalized result.
pub(crate) fn edit(app: &App, op: Op) {
    if app.guard_read_only() {
        return;
    }
    let (id, cursor) = {
        let st = app.state.borrow();
        match st.player.current {
            Some(id) => (id, st.player.cursor),
            None => return,
        }
    };
    let (phrases, grid) = {
        let st = app.state.borrow();
        let a = st.lib.analysis(id).unwrap_or_default();
        (a.phrases, a.beatgrid)
    };
    let beat = edit_beat(&grid, cursor);
    // A time picked on the strip: the beat inside the phrase there (the
    // block's middle need not be on a bar).
    let inside = |t: f64| grid.beat_position(t).map(|b| b.floor().max(0.0) as u32);
    let (next, label) = match (op, beat) {
        (Op::KindAt(t, k), _) => match inside(t) {
            Some(b) => (set_kind(&phrases, b, k, grid.beats.len() as u32), "Phrase changed"),
            None => {
                app.status("This track has no beatgrid yet — analyse it first");
                return;
            }
        },
        (Op::MergeAt(t), _) => match inside(t).and_then(|b| merge(&phrases, b)) {
            Some(v) => (v, "Phrases merged"),
            None => {
                app.status("No phrase follows to merge with");
                return;
            }
        },
        (Op::MoveEdge(from, to), _) => match (edit_beat(&grid, from), edit_beat(&grid, to)) {
            (Some(a), Some(b)) => match move_edge(&phrases, a, b) {
                Some(v) => (v, "Phrase boundary moved"),
                None => {
                    app.status("The boundary cannot move past the next one");
                    return;
                }
            },
            _ => {
                app.status("This track has no beatgrid yet — analyse it first");
                return;
            }
        },
        (Op::Clear, _) => {
            if phrases.is_empty() {
                app.status("This track has no phrases");
                return;
            }
            (Vec::new(), "Phrases cleared")
        }
        (_, None) => {
            app.status("This track has no beatgrid yet — analyse it first");
            return;
        }
        (Op::Kind(k), Some(b)) => (
            set_kind(&phrases, b, k, grid.beats.len() as u32),
            "Phrase changed",
        ),
        (Op::Cut, Some(b)) => match cut(&phrases, b) {
            Some(v) => (v, "Phrase cut at the cursor"),
            None => {
                app.status(if phrase_at(&phrases, b).is_some() {
                    "Cut at least 4 bars from the edges of the phrase"
                } else {
                    "No phrase to cut at the cursor"
                });
                return;
            }
        },
        (Op::Merge, Some(b)) => match merge(&phrases, b) {
            Some(v) => (v, "Phrases merged"),
            None => {
                app.status("No phrase follows to merge with");
                return;
            }
        },
        (Op::Boundary, Some(b)) => match move_boundary(&phrases, b) {
            Some(v) => (v, "Phrase boundary moved to the cursor"),
            None => {
                app.status("The boundary cannot move past the next one");
                return;
            }
        },
    };
    let normalized = normalize_phrases(&next);
    if normalized == phrases {
        app.status("Phrases shorter than 4 bars join their neighbour — nothing changed");
        return;
    }
    let r = app.state.borrow_mut().lib.set_phrases(id, &normalized);
    if app.report(r).is_some() {
        app.after_edit(label);
    }
}

/// The key for the type picker: opens it beside the phrase at the cursor.
pub(crate) fn open_picker(app: &App) {
    let st = app.state.borrow();
    let Some(id) = st.player.current else { return };
    let a = st.lib.analysis(id).unwrap_or_default();
    let mut phrases = a.phrases.clone();
    phrases.sort_by_key(|p| p.start_beat);
    let Some(i) = a
        .beatgrid
        .beat_position(st.player.cursor)
        .map(|b| b.floor().max(0.0) as u32)
        .and_then(|b| phrase_at(&phrases, b))
    else {
        drop(st);
        app.status("No phrase at the cursor");
        return;
    };
    drop(st);
    let pe = app.ui.global::<crate::PhraseEdit>();
    pe.set_picker_index(i as i32);
    pe.set_picker_request(pe.get_picker_request() + 1);
}

/// The phrase at the cursor for the PHRASE tab ("" = none).
pub(crate) fn current_label(phrases: &[Phrase], grid: &Beatgrid, cursor: f64) -> &'static str {
    let Some(b) = grid
        .beat_position(cursor)
        .map(|b| b.floor().max(0.0) as u32)
    else {
        return "";
    };
    phrase_at(phrases, b)
        .map(|i| crate::render::phrase_label(phrases[i].kind))
        .unwrap_or("")
}

pub(crate) fn wire(app: &Rc<App>) {
    let pe = app.ui.global::<crate::PhraseEdit>();
    pe.set_kinds(crate::models::model(
        KINDS
            .iter()
            .map(|k| slint::SharedString::from(crate::render::phrase_label(*k)))
            .collect(),
    ));
    pe.on_kind(handler!(app, |a, i: i32| {
        if let Some(k) = KINDS.get(i.max(0) as usize) {
            edit(&a, Op::Kind(*k));
        }
    }));
    pe.on_cut(handler!(app, |a| edit(&a, Op::Cut)));
    pe.on_merge(handler!(app, |a| edit(&a, Op::Merge)));
    // On the strip: positions as fractions of the track.
    let secs = |a: &App, f: f32| {
        let st = a.state.borrow();
        let d = st.player.current.and_then(|id| st.lib.track(id).ok()).map(|t| t.duration).unwrap_or(0.0);
        f.clamp(0.0, 1.0) as f64 * d
    };
    pe.on_kind_at(handler!(app, |a, f: f32, i: i32| {
        if let Some(k) = KINDS.get(i.max(0) as usize) {
            let t = secs(&a, f);
            edit(&a, Op::KindAt(t, *k));
        }
    }));
    pe.on_merge_at(handler!(app, |a, f: f32| {
        let t = secs(&a, f);
        edit(&a, Op::MergeAt(t));
    }));
    pe.on_move_edge(handler!(app, |a, from: f32, to: f32| {
        let (from, to) = (secs(&a, from), secs(&a, to));
        edit(&a, Op::MoveEdge(from, to));
    }));
    // Where a drag would land: the bar the edge snaps to, as a fraction.
    let weak = Rc::downgrade(app);
    pe.on_snap(move |f: f32| -> f32 {
        let Some(a) = weak.upgrade() else { return f };
        let st = a.state.borrow();
        let Some(id) = st.player.current else { return f };
        let d = st.lib.track(id).map(|t| t.duration).unwrap_or(0.0);
        let grid = st.lib.analysis(id).unwrap_or_default().beatgrid;
        match (d > 0.0, edit_beat(&grid, f.clamp(0.0, 1.0) as f64 * d)) {
            (true, Some(b)) => grid.beats.get(b as usize).map(|x| (x.time / d) as f32).unwrap_or(f),
            _ => f,
        }
    });
    pe.on_boundary(handler!(app, |a| edit(&a, Op::Boundary)));
    pe.on_clear(handler!(app, |a| edit(&a, Op::Clear)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use PhraseKind::*;

    fn p(s: u32, e: u32, kind: PhraseKind) -> Phrase {
        Phrase {
            start_beat: s * 4,
            end_beat: e * 4,
            kind,
        }
    }

    #[test]
    fn kind_changes_and_fills_gaps() {
        let v = [p(0, 16, Intro), p(32, 64, Chorus)];
        assert_eq!(set_kind(&v, 40 * 4, Verse, 256)[1].kind, Verse);
        // In the gap between the two: a phrase fills exactly the gap.
        let g = set_kind(&v, 20 * 4, Bridge, 256);
        assert_eq!(g[1], p(16, 32, Bridge));
        // After the last one: up to the end of the grid; without phrases: all of it.
        assert_eq!(
            set_kind(&v, 70 * 4, Outro, 400)[2],
            Phrase {
                start_beat: 64 * 4,
                end_beat: 400,
                kind: Outro
            }
        );
        assert_eq!(
            set_kind(&[], 10, Up, 128),
            vec![Phrase {
                start_beat: 0,
                end_beat: 128,
                kind: Up
            }]
        );
    }

    #[test]
    fn cuts_keep_a_different_kind_and_survive_normalization() {
        let v = [p(0, 32, Intro), p(32, 64, Verse)];
        let c = cut(&v, 16 * 4).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].end_beat, c[1].start_beat), (64, 64));
        assert!(
            c[1].kind != Intro && c[1].kind != Verse,
            "the new part differs from both neighbours"
        );
        assert_eq!(
            normalize_phrases(&c).len(),
            3,
            "a 16-bar part stays its own phrase"
        );
        assert!(cut(&v, 32 * 4).is_none(), "already a boundary");
        assert!(cut(&v, 80 * 4).is_none(), "no phrase there");
        // A part under 4 bars would join its neighbour: no cut there.
        assert!(cut(&v, 30 * 4).is_none() && cut(&v, 2 * 4).is_none());
        assert!(
            cut(&v, 4 * 4).is_some() && cut(&v, 28 * 4).is_some(),
            "exactly 4 bars is enough"
        );
    }

    #[test]
    fn boundaries_move_without_crossing() {
        let v = [p(0, 16, Intro), p(16, 48, Chorus), p(48, 64, Outro)];
        let m = move_boundary(&v, 20 * 4).unwrap();
        assert_eq!(
            (m[0].end_beat, m[1].start_beat),
            (80, 80),
            "the shared boundary moves for both phrases"
        );
        assert_eq!(m[2], v[2]);
        // The end of the track moves too.
        assert_eq!(move_boundary(&v, 70 * 4).unwrap()[2].end_beat, 280);
        assert!(move_boundary(&v, 16 * 4).is_none(), "already there");
        assert!(move_boundary(&[], 8).is_none());
        // The nearest boundary moves, never one beyond it.
        let m = move_boundary(&v, 40 * 4).unwrap();
        assert_eq!((m[1].end_beat, m[2].start_beat), (160, 160));
        assert_eq!(m[0], v[0]);
    }

    #[test]
    fn edges_move_and_phrases_merge() {
        // Bars (p takes bars; edits take beats).
        let v = [p(0, 8, Intro), p(8, 16, Verse), p(16, 32, Outro)];
        // The edge near beat 33 (bar 8) goes to beat 48 (bar 12).
        assert_eq!(move_edge(&v, 33, 48).unwrap(), [p(0, 12, Intro), p(12, 16, Verse), p(16, 32, Outro)]);
        // Not past the next boundary (bar 16 = beat 64).
        assert!(move_edge(&v, 32, 70).is_none());
        // The verse swallows the outro; the last phrase has nothing to merge.
        assert_eq!(merge(&v, 40).unwrap(), [p(0, 8, Intro), p(8, 32, Verse)]);
        assert!(merge(&v, 100).is_none());
    }

    #[test]
    fn edits_snap_to_the_nearest_downbeat() {
        let g = Beatgrid::constant(120.0, 0.0, 1, 120.0);
        // Beat 9 (4.5 s) is one beat after the downbeat at beat 8.
        assert_eq!(edit_beat(&g, 4.5), Some(8));
        assert_eq!(
            edit_beat(&g, 5.6),
            Some(12),
            "beat 11 → the next downbeat (12)"
        );
        assert_eq!(edit_beat(&Beatgrid::default(), 3.0), None);
        let v = [p(0, 16, Intro), p(16, 48, Chorus)];
        assert_eq!(current_label(&v, &g, 1.0), "INTRO");
        assert_eq!(current_label(&v, &g, 40.0), "CHORUS");
        assert_eq!(current_label(&v, &g, 100.0), "");
    }
}
