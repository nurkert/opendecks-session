//! Phrase (section) detection and phrase mood.
//!
//! Works on bars taken from the beatgrid (downbeat to downbeat):
//!
//! 1. Per bar, mean energy in dB of the full band, the low band (< 150 Hz),
//!    the mid band and the high band (> 2 kHz).
//! 2. A novelty value at every bar boundary: the L1 distance between the mean
//!    features of the four bars before and after it, plus the plain
//!    bar-to-bar change.
//! 3. Dynamic programming places phrase boundaries: phrases are 8, 16 or 32
//!    bars long (the last one may be shorter), each boundary earns its
//!    novelty minus a threshold (the median novelty at 8-bar positions), so
//!    boundaries land where the arrangement actually changes.
//! 4. Heuristic labelling from relative energy, bass presence and energy
//!    slope: Intro / Outro at the edges, Chorus for high-energy phrases with
//!    bass, Up for rising phrases leading into a Chorus, Down for breakdowns
//!    (low energy or no bass), Verse before the first Chorus and Bridge
//!    after it. Neighbouring phrases of the same kind are merged into one.
//! 5. The mood (High / Mid / Low) follows from tempo, share of Chorus bars
//!    and bass weight.

use crate::dsp::{Biquad, energy_prefix, filtered, mean_square};
use od_core::{Beatgrid, Phrase, PhraseKind, PhraseMood};
use std::f64::consts::FRAC_1_SQRT_2;

/// Allowed regular phrase lengths in bars.
const LENGTHS: [usize; 3] = [8, 16, 32];

#[derive(Clone, Copy, Debug)]
struct BarFeat {
    all: f64,
    low: f64,
    mid: f64,
    high: f64,
    /// Linear mean square of the full band.
    power: f64,
}

impl BarFeat {
    fn dist(&self, o: &BarFeat) -> f64 {
        (self.all - o.all).abs()
            + (self.low - o.low).abs()
            + (self.mid - o.mid).abs()
            + (self.high - o.high).abs()
    }
}

fn mean_feat(f: &[BarFeat]) -> BarFeat {
    let n = f.len().max(1) as f64;
    let mut m = BarFeat {
        all: 0.0,
        low: 0.0,
        mid: 0.0,
        high: 0.0,
        power: 0.0,
    };
    for b in f {
        m.all += b.all / n;
        m.low += b.low / n;
        m.mid += b.mid / n;
        m.high += b.high / n;
        m.power += b.power / n;
    }
    m
}

/// Detects phrases from a mono signal and its beatgrid. Returns no phrases
/// (and [`PhraseMood::Mid`]) when the grid has fewer than eight bars.
pub fn detect_phrases(
    samples: &[f32],
    sample_rate: u32,
    grid: &Beatgrid,
) -> (Vec<Phrase>, PhraseMood) {
    let none = (Vec::new(), PhraseMood::default());
    if samples.is_empty() || sample_rate == 0 || grid.beats.len() < 32 {
        return none;
    }
    let fs = sample_rate as f64;
    let q = FRAC_1_SQRT_2;
    let lp = Biquad::lowpass(fs, 150.0, q);
    let hp_low = Biquad::highpass(fs, 150.0, q);
    let hi_f = 2000.0f64.min(fs * 0.4);
    let lp_high = Biquad::lowpass(fs, hi_f, q);
    let hp_high = Biquad::highpass(fs, hi_f, q);
    let ((p_all, p_low), (p_mid, p_high)) = rayon::join(
        || {
            rayon::join(
                || energy_prefix(samples),
                || energy_prefix(&filtered(samples, &[lp, lp])),
            )
        },
        || {
            rayon::join(
                || energy_prefix(&filtered(samples, &[hp_low, hp_low, lp_high, lp_high])),
                || energy_prefix(&filtered(samples, &[hp_high, hp_high])),
            )
        },
    );

    // Bars: (first beat index, start sample, end sample).
    let downbeats: Vec<usize> = grid
        .beats
        .iter()
        .enumerate()
        .filter(|(_, b)| b.bar_position == 1)
        .map(|(i, _)| i)
        .collect();
    let n_samples = samples.len();
    let mut bars: Vec<(usize, BarFeat)> = Vec::new();
    for &d in &downbeats {
        let Some(end_beat) = grid.beats.get(d + 4) else {
            break;
        };
        let a = ((grid.beats[d].time * fs).max(0.0) as usize).min(n_samples);
        let b = ((end_beat.time * fs) as usize).min(n_samples);
        if b <= a {
            break;
        }
        let db = |p: &[f64]| 10.0 * (mean_square(p, a, b) + 1e-10).log10();
        bars.push((
            d,
            BarFeat {
                all: db(&p_all),
                low: db(&p_low),
                mid: db(&p_mid),
                high: db(&p_high),
                power: mean_square(&p_all, a, b),
            },
        ));
    }
    let max_power = bars.iter().map(|b| b.1.power).fold(0.0, f64::max);
    if bars.len() < 8 || max_power <= 1e-10 {
        return none;
    }
    // Music range: first to last bar within 30 dB of the loudest bar.
    let audible = |f: &BarFeat| f.power > max_power * 1e-3;
    let Some(first) = bars.iter().position(|b| audible(&b.1)) else {
        return none;
    };
    let last = bars
        .iter()
        .rposition(|b| audible(&b.1))
        .unwrap_or(bars.len() - 1);
    let bars = &bars[first..=last];
    let feats: Vec<BarFeat> = bars
        .iter()
        .map(|b| {
            // Clip at −60 dB below the loudest bar so silence doesn't dominate.
            let floor = 10.0 * max_power.log10() - 60.0;
            BarFeat {
                all: b.1.all.max(floor),
                low: b.1.low.max(floor),
                mid: b.1.mid.max(floor),
                high: b.1.high.max(floor),
                power: b.1.power,
            }
        })
        .collect();
    let nb = feats.len();

    let novelty: Vec<f64> = (0..=nb)
        .map(|j| {
            if j == 0 || j >= nb {
                return 0.0;
            }
            let w = 4.min(j).min(nb - j);
            mean_feat(&feats[j - w..j]).dist(&mean_feat(&feats[j..j + w]))
                + 0.5 * feats[j].dist(&feats[j - 1])
        })
        .collect();

    let mut boundaries = segment(&novelty, nb);
    // A trailing fragment shorter than four bars joins the previous phrase.
    if boundaries.len() > 2 && nb - boundaries[boundaries.len() - 2] < 4 {
        boundaries.remove(boundaries.len() - 2);
    }
    let end_beat_of = |bar: usize| -> u32 {
        match bars.get(bar) {
            Some(b) => b.0 as u32,
            None => (bars[nb - 1].0 + 4).min(grid.beats.len()) as u32,
        }
    };

    // Per-phrase statistics.
    struct Stats {
        energy: f64,
        low: f64,
        slope: f64,
    }
    let stats: Vec<Stats> = boundaries
        .windows(2)
        .map(|w| {
            let f = &feats[w[0]..w[1]];
            let m = mean_feat(f);
            let h = f.len() / 2;
            let slope = if h > 0 {
                let a = mean_feat(&f[..h]);
                let b = mean_feat(&f[h..]);
                (b.all - a.all) + (b.high - a.high)
            } else {
                0.0
            };
            Stats {
                energy: m.all,
                low: m.low,
                slope,
            }
        })
        .collect();
    let e_max = stats.iter().map(|s| s.energy).fold(f64::MIN, f64::max);
    let e_min = stats.iter().map(|s| s.energy).fold(f64::MAX, f64::min);
    let low_max = stats.iter().map(|s| s.low).fold(f64::MIN, f64::max);
    let range = e_max - e_min;
    let norm = |e: f64| {
        if range < 3.0 {
            1.0
        } else {
            (e - e_min) / range
        }
    };
    let n = stats.len();
    let hi: Vec<bool> = stats
        .iter()
        .map(|s| norm(s.energy) >= 0.75 && s.low >= low_max - 6.0)
        .collect();
    let mut kinds = Vec::with_capacity(n);
    let mut seen_chorus = false;
    for (i, s) in stats.iter().enumerate() {
        let en = norm(s.energy);
        let bass = s.low >= low_max - 9.0;
        let kind = if n >= 3 && i == 0 {
            PhraseKind::Intro
        } else if n >= 3 && i == n - 1 {
            PhraseKind::Outro
        } else if n >= 4 && i == 1 && kinds[0] == PhraseKind::Intro && en < 0.5 && !hi[i] {
            PhraseKind::Intro
        } else if hi[i] || (kinds.last() == Some(&PhraseKind::Chorus) && en >= 0.6 && bass) {
            // Hysteresis: a chorus stays a chorus until the energy clearly
            // drops, so phrases near the threshold do not flap.
            PhraseKind::Chorus
        } else if hi.get(i + 1).copied().unwrap_or(false) && s.slope > 1.0 {
            // A riser into the drop is Up even without bass.
            PhraseKind::Up
        } else if en < 0.4 || !bass {
            PhraseKind::Down
        } else if seen_chorus {
            PhraseKind::Bridge
        } else {
            PhraseKind::Verse
        };
        seen_chorus |= kind == PhraseKind::Chorus;
        kinds.push(kind);
    }
    // Neighbouring phrases of the same kind are one section. The segmentation
    // cuts wherever the texture shifts, but a drop that loses its hats halfway
    // is still one Chorus, not Chorus, Chorus, Chorus.
    let mut i = 1;
    while i < kinds.len() {
        if kinds[i] == kinds[i - 1] {
            kinds.remove(i);
            boundaries.remove(i);
        } else {
            i += 1;
        }
    }

    let phrases: Vec<Phrase> = od_core::normalize_phrases(
        &boundaries
            .windows(2)
            .zip(&kinds)
            .map(|(w, &kind)| Phrase {
                start_beat: end_beat_of(w[0]),
                end_beat: end_beat_of(w[1]),
                kind,
            })
            .collect::<Vec<_>>(),
    );

    // Mood.
    let bpm = grid.dominant_bpm().unwrap_or(120.0);
    let chorus_bars: usize = boundaries
        .windows(2)
        .zip(&kinds)
        .filter(|(_, k)| matches!(k, PhraseKind::Chorus | PhraseKind::Up))
        .map(|(w, _)| w[1] - w[0])
        .sum();
    let chorus_share = chorus_bars as f64 / nb as f64;
    let mean_bass = feats.iter().map(|f| f.low - f.all).sum::<f64>() / nb as f64;
    let mood = if bpm >= 118.0 && chorus_share >= 0.3 && mean_bass > -8.0 {
        PhraseMood::High
    } else if bpm < 100.0 || (chorus_share < 0.15 && range >= 3.0) {
        PhraseMood::Low
    } else {
        PhraseMood::Mid
    };
    (phrases, mood)
}

/// Dynamic-programming segmentation of `nb` bars into phrases. Returns the
/// boundary bar indices including 0 and `nb`.
fn segment(novelty: &[f64], nb: usize) -> Vec<usize> {
    if nb <= 8 {
        return vec![0, nb];
    }
    let mut at8: Vec<f64> = (8..nb).step_by(8).map(|j| novelty[j]).collect();
    at8.sort_by(f64::total_cmp);
    let lambda = at8.get(at8.len() / 2).copied().unwrap_or(0.0);
    let mut best = vec![f64::NEG_INFINITY; nb + 1];
    let mut prev = vec![usize::MAX; nb + 1];
    best[0] = 0.0;
    for p in 1..=nb {
        let gain = if p == nb { 0.0 } else { novelty[p] - lambda };
        let mut cands: Vec<(usize, f64)> = LENGTHS
            .iter()
            .filter(|&&l| l <= p)
            .map(|&l| (p - l, 0.0))
            .collect();
        if p == nb {
            // The final phrase may have any length up to 32 bars.
            cands.extend(
                (1..=32.min(p))
                    .filter(|l| !LENGTHS.contains(l))
                    .map(|l| (p - l, 0.5 * lambda.abs() + 0.1)),
            );
        }
        for (from, penalty) in cands {
            if best[from].is_finite() {
                let v = best[from] + gain - penalty;
                if v > best[p] {
                    best[p] = v;
                    prev[p] = from;
                }
            }
        }
    }
    let mut out = vec![nb];
    let mut p = nb;
    while p > 0 {
        let q = prev[p];
        if q == usize::MAX {
            return vec![0, nb];
        }
        out.push(q);
        p = q;
    }
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsig::*;
    use std::f64::consts::PI;

    const FS: u32 = 11_025;

    /// Arrangement at 128 BPM, 4 beats per bar, given as (bars, layers) with
    /// layers = (kick, bass, hats, pad level, riser).
    fn arrangement(sections: &[(usize, bool, bool, bool, f64, bool)]) -> (Vec<f32>, Beatgrid) {
        let bpm = 128.0;
        let period = 60.0 / bpm;
        let total_bars: usize = sections.iter().map(|s| s.0).sum();
        let secs = total_bars as f64 * 4.0 * period + 1.0;
        let mut x = vec![0f32; (secs * FS as f64) as usize];
        let k = kick(FS, 0.7);
        let hat = burst(FS, 0.2, 0.02, 9);
        let mut noise = Noise::new(42);
        let mut bar0 = 0;
        for &(bars, kick_on, bass_on, hats_on, pad, riser) in sections {
            for b in 0..bars {
                for beat in 0..4 {
                    let t = ((bar0 + b) * 4 + beat) as f64 * period;
                    let i = (t * FS as f64) as usize;
                    if kick_on {
                        add(&mut x, i, &k);
                    }
                    if hats_on {
                        add(&mut x, i + (period / 2.0 * FS as f64) as usize, &hat);
                    }
                    let n = (period * FS as f64) as usize;
                    for j in 0..n {
                        let tt = j as f64 / FS as f64;
                        let mut v = 0.0;
                        if bass_on {
                            v += 0.25 * (2.0 * PI * 55.0 * tt).sin();
                        }
                        if pad > 0.0 {
                            for f in [220.0, 261.6, 329.6] {
                                v += pad * (2.0 * PI * f * (t + tt)).sin();
                            }
                        }
                        if riser {
                            let prog = (b * 4 + beat) as f64 / (bars * 4) as f64;
                            v += 0.3 * prog * noise.next();
                        }
                        if let Some(s) = x.get_mut(i + j) {
                            *s += v as f32;
                        }
                    }
                }
            }
            bar0 += bars;
        }
        let grid = Beatgrid::constant(bpm, 0.0, 1, secs);
        (x, grid)
    }

    #[test]
    fn edm_arrangement_is_segmented_and_labelled() {
        let (x, grid) = arrangement(&[
            (16, true, false, false, 0.0, false),  // intro: kick only
            (16, true, true, false, 0.0, false),   // verse: kick + bass
            (8, false, false, false, 0.05, false), // breakdown: quiet pad
            (8, false, false, false, 0.05, true),  // build-up: riser
            (32, true, true, true, 0.08, false),   // drop
            (16, true, false, false, 0.0, false),  // outro
        ]);
        let (phrases, mood) = detect_phrases(&x, FS, &grid);
        assert!(!phrases.is_empty());
        // Contiguous, bar aligned, whole 8-bar blocks except possibly the last.
        for w in phrases.windows(2) {
            assert_eq!(w[0].end_beat, w[1].start_beat);
        }
        for p in &phrases[..phrases.len() - 1] {
            let bars = (p.end_beat - p.start_beat) / 4;
            assert!(bars > 0 && bars % 8 == 0, "{phrases:?}");
            assert_eq!(p.start_beat % 4, 0);
        }
        let kind_at = |bar: u32| {
            phrases
                .iter()
                .find(|p| p.start_beat <= bar * 4 && bar * 4 < p.end_beat)
                .map(|p| p.kind)
        };
        let starts: Vec<u32> = phrases.iter().map(|p| p.start_beat / 4).collect();
        for b in [32, 40, 48, 80] {
            assert!(
                starts.contains(&b),
                "boundary at bar {b} missing: {starts:?}"
            );
        }
        assert_eq!(phrases[0].kind, PhraseKind::Intro);
        assert_eq!(phrases.last().unwrap().kind, PhraseKind::Outro);
        assert_eq!(kind_at(35), Some(PhraseKind::Down), "{phrases:?}");
        assert_eq!(kind_at(60), Some(PhraseKind::Chorus), "{phrases:?}");
        assert_eq!(kind_at(70), Some(PhraseKind::Chorus), "{phrases:?}");
        assert_eq!(mood, PhraseMood::High);
    }

    #[test]
    fn same_kind_neighbours_are_merged() {
        let (x, grid) = arrangement(&[
            (16, true, false, false, 0.0, false),  // intro
            (16, true, true, true, 0.08, false),   // drop
            (16, true, true, false, 0.08, false),  // drop without hats
            (8, false, false, false, 0.05, false), // breakdown
            (16, true, true, true, 0.08, false),   // drop
            (16, true, false, false, 0.0, false),  // outro
        ]);
        let (phrases, _) = detect_phrases(&x, FS, &grid);
        for w in phrases.windows(2) {
            assert_ne!(w[0].kind, w[1].kind, "{phrases:?}");
        }
        let first_drop = phrases
            .iter()
            .find(|p| p.kind == PhraseKind::Chorus)
            .unwrap();
        assert_eq!(
            (first_drop.start_beat / 4, first_drop.end_beat / 4),
            (16, 48),
            "{phrases:?}"
        );
    }

    #[test]
    fn too_short_or_silent() {
        let grid = Beatgrid::constant(128.0, 0.0, 1, 10.0);
        assert!(
            detect_phrases(&vec![0.1; FS as usize * 10], FS, &grid)
                .0
                .is_empty()
        );
        let grid = Beatgrid::constant(128.0, 0.0, 1, 60.0);
        assert!(
            detect_phrases(&vec![0.0; FS as usize * 60], FS, &grid)
                .0
                .is_empty()
        );
        assert!(detect_phrases(&[], FS, &Beatgrid::default()).0.is_empty());
    }

    #[test]
    fn segmentation_prefers_novel_boundaries() {
        let mut nov = vec![0.1; 65];
        nov[24] = 5.0;
        let b = segment(&nov, 64);
        assert!(b.contains(&24), "{b:?}");
        assert_eq!(*b.last().unwrap(), 64);
    }
}
