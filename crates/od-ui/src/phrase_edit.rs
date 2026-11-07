//! Phrase editing in the Prepare player (PHRASE tab next to GRID): the kind
//! of the phrase at the cursor, a cut at the cursor, the nearest boundary
//! moved to the cursor, and clearing all phrases. Every edit is stored
//! through the library (one undo step, normalized with
//! [`od_core::normalize_phrases`]) and reaches registered sticks like any
//! other edit.

use crate::app::{App, handler};
use od_core::{Beatgrid, Phrase, PhraseKind, normalize_phrases};
use slint::ComponentHandle;
use std::rc::Rc;

/// The kinds offered in the PHRASE tab, in button order.
pub(crate) const KINDS: [PhraseKind; 7] =
    [PhraseKind::Intro, PhraseKind::Verse, PhraseKind::Chorus, PhraseKind::Bridge, PhraseKind::Up, PhraseKind::Down, PhraseKind::Outro];

/// The beat a phrase edit at `t` seconds applies to: the nearest downbeat
/// within two beats (phrases start on bars), else the nearest beat. `None`
/// without a grid (pure).
pub(crate) fn edit_beat(grid: &Beatgrid, t: f64) -> Option<u32> {
    let n = grid.beats.len();
    if n == 0 {
        return None;
    }
    let i = grid.beat_position(t).map(|b| b.round().clamp(0.0, (n - 1) as f64) as usize).unwrap_or(0);
    let down = (i.saturating_sub(2)..=(i + 2).min(n - 1))
        .filter(|&j| grid.beats[j].bar_position == 1)
        .min_by(|a, b| (grid.beats[*a].time - t).abs().total_cmp(&(grid.beats[*b].time - t).abs()));
    Some(down.unwrap_or(i) as u32)
}

/// Index of the phrase that holds `beat`.
pub(crate) fn phrase_at(phrases: &[Phrase], beat: u32) -> Option<usize> {
    phrases.iter().position(|p| p.start_beat <= beat && beat < p.end_beat)
}

/// The phrase at `beat` takes `kind`; in a gap (or without phrases) a new
/// phrase of `kind` fills the gap around `beat` up to `total` beats (pure,
/// not normalized).
pub(crate) fn set_kind(phrases: &[Phrase], beat: u32, kind: PhraseKind, total: u32) -> Vec<Phrase> {
    let mut v = sorted(phrases);
    match phrase_at(&v, beat) {
        Some(i) => v[i].kind = kind,
        None => {
            let start = v.iter().filter(|p| p.end_beat <= beat).map(|p| p.end_beat).max().unwrap_or(0);
            let end = v.iter().filter(|p| p.start_beat > beat).map(|p| p.start_beat).min().unwrap_or(total);
            if end > start {
                v.push(Phrase { start_beat: start, end_beat: end, kind });
                v.sort_by_key(|p| p.start_beat);
            }
        }
    }
    v
}

/// Splits the phrase at `beat` there; the part from `beat` on takes the
/// next kind that differs from the phrase and from the one after it (so
/// the cut survives normalization). `None` when no phrase holds `beat` or
/// `beat` already is a boundary (pure).
pub(crate) fn cut(phrases: &[Phrase], beat: u32) -> Option<Vec<Phrase>> {
    let mut v = sorted(phrases);
    let i = phrase_at(&v, beat).filter(|&i| v[i].start_beat < beat)?;
    let kind = v[i].kind;
    let following = v.get(i + 1).filter(|n| n.start_beat == v[i].end_beat).map(|n| n.kind);
    let at = KINDS.iter().position(|k| *k == kind).unwrap_or(0);
    let next = (1..KINDS.len()).map(|d| KINDS[(at + d) % KINDS.len()]).find(|k| Some(*k) != following).unwrap_or(kind);
    let tail = Phrase { start_beat: beat, end_beat: v[i].end_beat, kind: next };
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
            start_beat: if p.start_beat == b { beat } else { p.start_beat },
            end_beat: if p.end_beat == b { beat } else { p.end_beat },
            kind: p.kind,
        })
        .collect();
    // Every phrase keeps its length and order.
    let ordered = out.iter().all(|p| p.end_beat > p.start_beat) && out.windows(2).all(|w| w[0].end_beat <= w[1].start_beat);
    ordered.then_some(out)
}

fn sorted(phrases: &[Phrase]) -> Vec<Phrase> {
    let mut v = phrases.to_vec();
    v.sort_by_key(|p| p.start_beat);
    v
}

/// One phrase edit at the cursor.
pub(crate) enum Op {
    Kind(PhraseKind),
    Cut,
    Boundary,
    Clear,
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
    let (next, label) = match (op, beat) {
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
        (Op::Kind(k), Some(b)) => (set_kind(&phrases, b, k, grid.beats.len() as u32), "Phrase changed"),
        (Op::Cut, Some(b)) => match cut(&phrases, b) {
            Some(v) => (v, "Phrase cut at the cursor"),
            None => {
                app.status("No phrase to cut at the cursor");
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

/// The phrase at the cursor for the PHRASE tab ("" = none).
pub(crate) fn current_label(phrases: &[Phrase], grid: &Beatgrid, cursor: f64) -> &'static str {
    let Some(b) = grid.beat_position(cursor).map(|b| b.floor().max(0.0) as u32) else { return "" };
    phrase_at(phrases, b).map(|i| crate::render::phrase_label(phrases[i].kind)).unwrap_or("")
}

pub(crate) fn wire(app: &Rc<App>) {
    let pe = app.ui.global::<crate::PhraseEdit>();
    pe.set_kinds(crate::models::model(KINDS.iter().map(|k| slint::SharedString::from(crate::render::phrase_label(*k))).collect()));
    pe.on_kind(handler!(app, |a, i: i32| {
        if let Some(k) = KINDS.get(i.max(0) as usize) {
            edit(&a, Op::Kind(*k));
        }
    }));
    pe.on_cut(handler!(app, |a| edit(&a, Op::Cut)));
    pe.on_boundary(handler!(app, |a| edit(&a, Op::Boundary)));
    pe.on_clear(handler!(app, |a| edit(&a, Op::Clear)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use PhraseKind::*;

    fn p(s: u32, e: u32, kind: PhraseKind) -> Phrase {
        Phrase { start_beat: s * 4, end_beat: e * 4, kind }
    }

    #[test]
    fn kind_changes_and_fills_gaps() {
        let v = [p(0, 16, Intro), p(32, 64, Chorus)];
        assert_eq!(set_kind(&v, 40 * 4, Verse, 256)[1].kind, Verse);
        // In the gap between the two: a phrase fills exactly the gap.
        let g = set_kind(&v, 20 * 4, Bridge, 256);
        assert_eq!(g[1], p(16, 32, Bridge));
        // After the last one: up to the end of the grid; without phrases: all of it.
        assert_eq!(set_kind(&v, 70 * 4, Outro, 256)[2], Phrase { start_beat: 64 * 4, end_beat: 256, kind: Outro });
        assert_eq!(set_kind(&[], 10, Up, 128), vec![Phrase { start_beat: 0, end_beat: 128, kind: Up }]);
    }

    #[test]
    fn cuts_keep_a_different_kind_and_survive_normalization() {
        let v = [p(0, 32, Intro), p(32, 64, Verse)];
        let c = cut(&v, 16 * 4).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].end_beat, c[1].start_beat), (64, 64));
        assert!(c[1].kind != Intro && c[1].kind != Verse, "the new part differs from both neighbours");
        assert_eq!(normalize_phrases(&c).len(), 3, "a 16-bar part stays its own phrase");
        assert!(cut(&v, 32 * 4).is_none(), "already a boundary");
        assert!(cut(&v, 80 * 4).is_none(), "no phrase there");
        // A two-bar fragment is absorbed again by normalization.
        assert_eq!(normalize_phrases(&cut(&v, 30 * 4).unwrap()), normalize_phrases(&v));
    }

    #[test]
    fn boundaries_move_without_crossing() {
        let v = [p(0, 16, Intro), p(16, 48, Chorus), p(48, 64, Outro)];
        let m = move_boundary(&v, 20 * 4).unwrap();
        assert_eq!((m[0].end_beat, m[1].start_beat), (80, 80), "the shared boundary moves for both phrases");
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
    fn edits_snap_to_the_nearest_downbeat() {
        let g = Beatgrid::constant(120.0, 0.0, 1, 120.0);
        // Beat 9 (4.5 s) is one beat after the downbeat at beat 8.
        assert_eq!(edit_beat(&g, 4.5), Some(8));
        assert_eq!(edit_beat(&g, 5.6), Some(12), "beat 11 → the next downbeat (12)");
        assert_eq!(edit_beat(&Beatgrid::default(), 3.0), None);
        let v = [p(0, 16, Intro), p(16, 48, Chorus)];
        assert_eq!(current_label(&v, &g, 1.0), "INTRO");
        assert_eq!(current_label(&v, &g, 40.0), "CHORUS");
        assert_eq!(current_label(&v, &g, 100.0), "");
    }
}
