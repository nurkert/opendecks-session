use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhraseKind {
    Intro,
    Verse,
    Chorus,
    Bridge,
    Up,
    Down,
    Outro,
}

/// Overall character used to colour phrases (mirrors the three moods used by
/// common DJ software: high, mid and low energy).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhraseMood {
    High,
    #[default]
    Mid,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    /// Index of the first beat of the phrase in the track's beatgrid.
    pub start_beat: u32,
    /// Index of the beat after the phrase.
    pub end_beat: u32,
    pub kind: PhraseKind,
}

/// Beats per bar assumed when phrase lengths are judged.
const BEATS_PER_BAR: u32 = 4;

/// Turns a phrase list into the sections a DJ reads: neighbours of the same
/// kind become one section, a short insert (under 8 bars) between two
/// phrases of the same kind is absorbed into them, and any remaining
/// fragment under 4 bars joins the phrase before it (or after it, at the
/// start). Phrases are sorted and gaps between them are left untouched.
///
/// Analysis, imported rekordbox phrases and the UI all pass through this, so
/// a drop that loses its hats for two bars stays one Chorus instead of
/// Chorus, Bridge, Chorus.
pub fn normalize_phrases(phrases: &[Phrase]) -> Vec<Phrase> {
    let mut v: Vec<Phrase> = phrases
        .iter()
        .copied()
        .filter(|p| p.end_beat > p.start_beat)
        .collect();
    v.sort_by_key(|p| p.start_beat);
    let bars = |p: &Phrase| (p.end_beat - p.start_beat) / BEATS_PER_BAR;
    loop {
        let before = v.len();
        // 1. Merge neighbours of the same kind.
        let mut out: Vec<Phrase> = Vec::with_capacity(v.len());
        for p in v.drain(..) {
            match out.last_mut() {
                Some(last) if last.kind == p.kind && last.end_beat >= p.start_beat => {
                    last.end_beat = p.end_beat
                }
                _ => out.push(p),
            }
        }
        v = out;
        // 2. A short insert between two equal phrases belongs to them.
        if let Some(i) = (1..v.len().saturating_sub(1)).find(|&i| {
            bars(&v[i]) < 8 && v[i - 1].kind == v[i + 1].kind && v[i].kind != v[i - 1].kind
        }) {
            v[i].kind = v[i - 1].kind;
            continue;
        }
        // 3. Fragments under four bars join a neighbour.
        if v.len() > 1
            && let Some(i) = (0..v.len()).find(|&i| bars(&v[i]) < 4)
        {
            let j = if i == 0 { 1 } else { i - 1 };
            v[i].kind = v[j].kind;
            continue;
        }
        if v.len() == before {
            return v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(start_bar: u32, end_bar: u32, kind: PhraseKind) -> Phrase {
        Phrase {
            start_beat: start_bar * 4,
            end_beat: end_bar * 4,
            kind,
        }
    }

    #[test]
    fn equal_neighbours_become_one_section() {
        use PhraseKind::*;
        let n = normalize_phrases(&[
            p(0, 16, Intro),
            p(16, 32, Chorus),
            p(32, 48, Chorus),
            p(48, 64, Chorus),
            p(64, 80, Outro),
        ]);
        assert_eq!(
            n,
            vec![p(0, 16, Intro), p(16, 64, Chorus), p(64, 80, Outro)]
        );
    }

    #[test]
    fn short_inserts_and_fragments_are_absorbed() {
        use PhraseKind::*;
        // Chorus, 4-bar Bridge, Chorus → one Chorus; a 2-bar Down after it joins it too.
        let n = normalize_phrases(&[
            p(0, 16, Intro),
            p(16, 32, Chorus),
            p(32, 36, Bridge),
            p(36, 52, Chorus),
            p(52, 54, Down),
            p(54, 70, Outro),
        ]);
        assert_eq!(
            n,
            vec![p(0, 16, Intro), p(16, 54, Chorus), p(54, 70, Outro)]
        );
    }

    #[test]
    fn real_sections_survive() {
        use PhraseKind::*;
        // A 16-bar breakdown between two choruses is a real section.
        let input = [
            p(0, 16, Intro),
            p(16, 48, Chorus),
            p(48, 64, Down),
            p(64, 96, Chorus),
            p(96, 112, Outro),
        ];
        assert_eq!(normalize_phrases(&input), input.to_vec());
    }
}
