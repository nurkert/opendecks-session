//! Tempo, beat phase and downbeat detection (constant-tempo beatgrid).
//!
//! Pipeline (all on the decimated analysis signal, ≈ 11 kHz):
//!
//! 1. **Onset strength** — log-compressed spectral flux with a short window
//!    (≈ 23 ms) and ≈ 3 ms hop, plus a low-band (< 150 Hz) flux used for
//!    downbeats. A moving-average is subtracted and the result rectified.
//! 2. **Coarse tempo** — autocorrelation of the onset envelope, scored for
//!    every candidate inside the folded BPM range at the beat lag and its
//!    2× / 4× multiples (bar regularity).
//! 3. **Fine tempo** — the magnitude of the onset envelope's Fourier
//!    transform at the beat frequency and its harmonics, maximised by a
//!    coarse-to-fine search over the whole track. This phase-aligns every
//!    beat of the track at once, giving sub-0.005 BPM precision on
//!    constant-tempo material.
//! 4. **Octave decision** — when both `b` and `2b` fit the range, the
//!    off-beat/on-beat ratio of the folded onset envelope decides.
//! 5. **Snapping** — the BPM snaps to an integer, .5, .1 or .01 value when
//!    that keeps the accumulated drift across the track below 2 ms
//!    (5 ms for integer tempos).
//! 6. **Phase** — the onset envelope folded at the final period gives a
//!    coarse beat position; it is refined with a sample-accurate energy-ratio
//!    onset envelope (forward/backward energy) folded at 0.5 ms resolution.
//! 7. **Downbeat** — per-beat low-band onset strength and beat-to-beat
//!    spectral-energy novelty are averaged for each of the four bar
//!    positions; the strongest wins.

use crate::AnalysisOptions;
use crate::dsp::{
    Biquad, decimate, decimation_factor, energy_prefix, mean_square, peak_abs, stft_map_pairs,
};
use od_core::Beatgrid;
use rayon::prelude::*;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::f64::consts::PI;

/// Analysis-rate target used for tempo and key.
pub(crate) fn analysis_rate_target(opts: &AnalysisOptions) -> u32 {
    if opts.lite { 8_000 } else { 11_025 }
}

/// Detects a constant-tempo beatgrid in a mono signal at any sample rate.
/// Returns an empty grid for silence or non-rhythmic material.
pub fn detect_beatgrid(samples: &[f32], sample_rate: u32, opts: &AnalysisOptions) -> Beatgrid {
    if sample_rate == 0 {
        return Beatgrid::default();
    }
    let factor = decimation_factor(sample_rate, analysis_rate_target(opts));
    let x = decimate(samples, factor);
    let duration = samples.len() as f64 / sample_rate as f64;
    detect_beatgrid_at(&x, sample_rate as f64 / factor as f64, duration, opts)
}

/// Beatgrid detection on a signal already at the analysis rate.
pub(crate) fn detect_beatgrid_at(
    x: &[f32],
    fs: f64,
    duration: f64,
    opts: &AnalysisOptions,
) -> Beatgrid {
    let (min_bpm, max_bpm) = sanitize_range(opts);
    let peak = peak_abs(x);
    if x.len() < (fs * 2.0) as usize || peak < 1e-4 {
        return Beatgrid::default();
    }
    let onsets = onset_envelopes(x, fs, peak, opts.lite);
    let o = &onsets.odf;
    let fr = onsets.frame_rate;
    if o.iter().all(|&v| v <= 0.0) {
        return Beatgrid::default();
    }

    // 2. Coarse tempo.
    let Some(coarse) = coarse_tempo(o, fr, min_bpm, max_bpm) else {
        return Beatgrid::default();
    };
    // 3. Fine tempo.
    let mut bpm = refine_tempo(o, fr, coarse, opts.lite);
    // 4. Octave decision.
    bpm = octave_decision(o, fr, bpm, min_bpm, max_bpm, opts.lite);
    // Rhythmic content check: enough beats must coincide with onsets.
    if !is_rhythmic(o, fr, bpm) {
        return Beatgrid::default();
    }
    // 5. Snapping.
    bpm = snap_bpm(bpm, duration);
    let period = 60.0 / bpm;
    // 6. Phase.
    let coarse_phase = folded_peak(o, fr, period);
    let phase = refine_phase(x, fs, period, coarse_phase);
    // 7. Downbeat.
    let first_bar_pos = downbeat_position(x, fs, &onsets, period, phase, duration);
    Beatgrid::constant(bpm, phase, first_bar_pos, duration)
}

fn sanitize_range(opts: &AnalysisOptions) -> (f64, f64) {
    let lo = if opts.min_bpm.is_finite() && opts.min_bpm > 20.0 {
        opts.min_bpm
    } else {
        87.0
    };
    let hi = if opts.max_bpm.is_finite() && opts.max_bpm > lo {
        opts.max_bpm
    } else {
        lo * 2.0
    };
    (lo, hi)
}

/// Onset envelopes at the frame rate `frame_rate`; frame `n` is centred on
/// time `n / frame_rate`.
pub(crate) struct Onsets {
    pub odf: Vec<f32>,
    pub low: Vec<f32>,
    pub frame_rate: f64,
}

pub(crate) fn onset_envelopes(x: &[f32], fs: f64, peak: f32, lite: bool) -> Onsets {
    let n_fft = if lite {
        256
    } else {
        (fs * 0.023).round().max(64.0) as usize
    }
    .next_power_of_two();
    let hop = ((fs * if lite { 0.006 } else { 0.0029 }).round() as usize).max(1);
    let scale = 100.0 / (peak * n_fft as f32 / 4.0);
    let bin_hz = fs / n_fft as f64;
    let low_bins = ((150.0 / bin_hz).ceil() as usize).max(2);
    let pairs: Vec<(f32, f32)> = stft_map_pairs(
        x,
        n_fft,
        hop,
        |m| m.iter_mut().for_each(|v| *v = (*v * scale).ln_1p()),
        |_, prev, cur| match prev {
            None => (0.0, 0.0),
            Some(p) => {
                let mut all = 0f32;
                let mut low = 0f32;
                for (k, (c, q)) in cur.iter().zip(p).enumerate().skip(1) {
                    let d = (c - q).max(0.0);
                    all += d;
                    if k <= low_bins {
                        low += d;
                    }
                }
                (all, low)
            }
        },
    );
    let frame_rate = fs / hop as f64;
    let (raw, low): (Vec<f32>, Vec<f32>) = pairs.into_iter().unzip();
    // Subtract a 0.25 s moving average and rectify.
    let w = ((frame_rate * 0.25) as usize).max(1);
    let mut prefix = Vec::with_capacity(raw.len() + 1);
    let mut acc = 0f64;
    prefix.push(0.0);
    for &v in &raw {
        acc += v as f64;
        prefix.push(acc);
    }
    let odf = (0..raw.len())
        .map(|i| {
            let a = i.saturating_sub(w);
            let b = (i + w + 1).min(raw.len());
            let mean = (prefix[b] - prefix[a]) / (b - a) as f64;
            (raw[i] as f64 - mean).max(0.0) as f32
        })
        .collect();
    Onsets {
        odf,
        low,
        frame_rate,
    }
}

/// Normalised autocorrelation of `o` up to `max_lag` (inclusive), via FFT.
fn autocorrelation(o: &[f32], max_lag: usize) -> Vec<f64> {
    let n = o.len();
    let size = (2 * n).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(size);
    let inv = planner.plan_fft_inverse(size);
    let mean = o.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let mut buf = fwd.make_input_vec();
    for (b, &v) in buf.iter_mut().zip(o) {
        *b = v as f64 - mean;
    }
    let mut spec = fwd.make_output_vec();
    if fwd.process(&mut buf, &mut spec).is_err() {
        return vec![0.0; max_lag + 1];
    }
    for c in spec.iter_mut() {
        *c = Complex::new(c.norm_sqr(), 0.0);
    }
    let mut out = inv.make_output_vec();
    if inv.process(&mut spec, &mut out).is_err() {
        return vec![0.0; max_lag + 1];
    }
    let r0 = out[0].max(1e-12);
    out.iter().take(max_lag + 1).map(|v| v / r0).collect()
}

/// Coarse tempo inside `[min_bpm, max_bpm]` from autocorrelation peaks.
fn coarse_tempo(o: &[f32], fr: f64, min_bpm: f64, max_bpm: f64) -> Option<f64> {
    let max_lag = (4.0 * 60.0 / min_bpm * fr).ceil() as usize + 3;
    if o.len() < max_lag + 2 {
        // Short signal: use what is available.
        if o.len() < (2.0 * 60.0 / min_bpm * fr) as usize {
            return None;
        }
    }
    let r = autocorrelation(o, max_lag.min(o.len() - 1));
    let at = |lag: f64| -> f64 {
        // Max of linearly interpolated values within ±1 frame (peaks are
        // narrow, so sampling exactly at the lag would be fragile).
        let mut best = f64::MIN;
        for d in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            let l = lag + d;
            let i = l.floor() as usize;
            if i + 1 >= r.len() {
                continue;
            }
            let f = l - i as f64;
            best = best.max(r[i] * (1.0 - f) + r[i + 1] * f);
        }
        if best == f64::MIN { 0.0 } else { best }
    };
    let steps = ((max_bpm - min_bpm) / 0.05).round() as usize;
    let mut best: Option<(f64, f64)> = None;
    for s in 0..=steps {
        let bpm = min_bpm + s as f64 * 0.05;
        let lag = 60.0 / bpm * fr;
        let score = at(lag) + 0.5 * at(2.0 * lag) + 0.25 * at(4.0 * lag);
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, bpm));
        }
    }
    best.filter(|(s, _)| *s > 0.0).map(|(_, b)| b)
}

/// Fourier coefficient of the onset envelope at `freq` Hz.
fn dft_at(o: &[f32], fr: f64, freq: f64) -> Complex<f64> {
    let (s, c) = (-2.0 * PI * freq / fr).sin_cos();
    let w = Complex::new(c, s);
    let mut z = Complex::new(1.0, 0.0);
    let mut acc = Complex::new(0.0, 0.0);
    for (n, &v) in o.iter().enumerate() {
        if v != 0.0 {
            acc += z * v as f64;
        }
        z *= w;
        if n & 1023 == 1023 {
            z /= z.norm();
        }
    }
    acc
}

/// Periodicity score of a tempo: summed Fourier magnitudes of the beat
/// frequency and its first harmonics.
fn tempo_score(o: &[f32], fr: f64, bpm: f64, harmonics: usize) -> f64 {
    (1..=harmonics)
        .map(|h| dft_at(o, fr, bpm / 60.0 * h as f64).norm())
        .sum()
}

/// Coarse-to-fine maximisation of [`tempo_score`] around `coarse`.
fn refine_tempo(o: &[f32], fr: f64, coarse: f64, lite: bool) -> f64 {
    let harmonics = if lite { 2 } else { 4 };
    let stages: &[(f64, f64)] = if lite {
        &[(1.0, 0.05), (0.06, 0.005), (0.006, 0.001)]
    } else {
        &[(1.0, 0.02), (0.04, 0.002), (0.004, 0.0002)]
    };
    let mut center = coarse;
    for &(span, step) in stages {
        let n = (span / step).round() as i64;
        let scores: Vec<(f64, f64)> = (-n..=n)
            .into_par_iter()
            .map(|k| {
                let b = center + k as f64 * step;
                (tempo_score(o, fr, b, harmonics), b)
            })
            .collect();
        if let Some(&(_, b)) = scores.iter().fold(None::<&(f64, f64)>, |acc, s| match acc {
            Some(a) if a.0 >= s.0 => Some(a),
            _ => Some(s),
        }) {
            center = b;
        }
    }
    center
}

/// Folds `o` at `period` seconds into `bins` phase bins (mean per bin).
fn fold(o: &[f32], fr: f64, period: f64, bins: usize) -> Vec<f64> {
    let mut sum = vec![0f64; bins];
    let mut cnt = vec![0u32; bins];
    for (n, &v) in o.iter().enumerate() {
        let t = n as f64 / fr;
        let ph = (t / period).fract();
        let b = ((ph * bins as f64) as usize).min(bins - 1);
        sum[b] += v as f64;
        cnt[b] += 1;
    }
    sum.iter()
        .zip(&cnt)
        .map(|(s, &c)| if c > 0 { s / c as f64 } else { 0.0 })
        .collect()
}

/// Circularly smooths with a [1, 2, 1] kernel.
fn smooth_circular(h: &[f64]) -> Vec<f64> {
    let n = h.len();
    (0..n)
        .map(|i| 0.25 * h[(i + n - 1) % n] + 0.5 * h[i] + 0.25 * h[(i + 1) % n])
        .collect()
}

/// Phase (seconds in `[0, period)`) of the strongest folded onset peak.
fn folded_peak(o: &[f32], fr: f64, period: f64) -> f64 {
    let bins = ((period * fr).round() as usize).clamp(16, 1024);
    let h = smooth_circular(&fold(o, fr, period, bins));
    let (i, _) = argmax(&h);
    (i as f64 + 0.5) / bins as f64 * period
}

fn argmax(h: &[f64]) -> (usize, f64) {
    h.iter()
        .enumerate()
        .fold((0, f64::MIN), |a, (i, &v)| if v > a.1 { (i, v) } else { a })
}

/// Chooses between `bpm` and its octave relative when both fit the range.
fn octave_decision(o: &[f32], fr: f64, bpm: f64, min_bpm: f64, max_bpm: f64, lite: bool) -> f64 {
    let tol = 0.5;
    let slower = if bpm * 2.0 <= max_bpm + tol {
        bpm
    } else if bpm / 2.0 >= min_bpm - tol {
        bpm / 2.0
    } else {
        return bpm;
    };
    // Refine the slower tempo so folding over the whole track stays sharp.
    let slower = refine_tempo(o, fr, slower, lite);
    let period = 60.0 / slower;
    let bins = 64;
    let h = smooth_circular(&fold(o, fr, period, bins));
    let (on, on_v) = argmax(&h);
    let off_v = (-2isize..=2)
        .map(|d| h[(on as isize + (bins / 2) as isize + d).rem_euclid(bins as isize) as usize])
        .fold(f64::MIN, f64::max);
    let mut sorted = h.clone();
    sorted.sort_by(f64::total_cmp);
    let base = sorted[bins / 2];
    let ratio = (off_v - base) / (on_v - base).max(1e-12);
    if ratio > 0.5 { slower * 2.0 } else { slower }
}

/// True when a reasonable share of beats coincides with an onset.
fn is_rhythmic(o: &[f32], fr: f64, bpm: f64) -> bool {
    let period = 60.0 / bpm;
    let phase = folded_peak(o, fr, period);
    let mut strengths = Vec::new();
    let mut t = phase;
    let dur = o.len() as f64 / fr;
    while t < dur {
        let c = (t * fr).round() as isize;
        let m = (c - 3..=c + 3)
            .filter(|&i| i >= 0 && (i as usize) < o.len())
            .map(|i| o[i as usize])
            .fold(0f32, f32::max);
        strengths.push(m);
        t += period;
    }
    if strengths.len() < 4 {
        return false;
    }
    let mut sorted = strengths.clone();
    sorted.sort_by(f32::total_cmp);
    let ref_v = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
    // Onset flux is level-normalised (log spectra relative to the peak), so
    // an absolute floor separates real transients from numerical ripple.
    if ref_v < 0.5 {
        return false;
    }
    let hits = strengths.iter().filter(|&&s| s >= 0.25 * ref_v).count();
    hits >= 4 && hits * 5 >= strengths.len()
}

/// Snaps the BPM to a "round" value if that keeps the drift across the whole
/// track (anchored in the middle) below 5 ms for integers and 2 ms for finer
/// steps; otherwise rounds to 1e-4.
fn snap_bpm(bpm: f64, duration: f64) -> f64 {
    let beats = (duration * bpm / 60.0).max(1.0);
    // Integer tempos are by far the most common in produced music, so they
    // get a slightly larger budget than finer steps.
    for (step, budget) in [(1.0, 0.005), (0.5, 0.003), (0.1, 0.002), (0.01, 0.002)] {
        let r = (bpm / step).round() * step;
        let drift = (60.0 / r - 60.0 / bpm).abs() * beats / 2.0;
        if drift <= budget {
            return (r * 1e4).round() / 1e4;
        }
    }
    (bpm * 1e4).round() / 1e4
}

/// Refines the beat phase with an energy-ratio onset envelope that peaks
/// exactly at transient starts, folded at ≈ 0.5 ms resolution within ±25 ms
/// of the coarse phase.
fn refine_phase(x: &[f32], fs: f64, period: f64, coarse: f64) -> f64 {
    let prefix = energy_prefix(x);
    let hop = ((fs * 0.0005).round() as usize).max(1);
    let wf = ((fs * 0.003).round() as usize).max(1);
    let wb = ((fs * 0.03).round() as usize).max(1);
    let eps = mean_square(&prefix, 0, x.len()) * 1e-4 + 1e-12;
    let bins = ((period * fs / hop as f64).round() as usize).max(8);
    let n_frames = x.len() / hop;
    let mut sum = vec![0f64; bins];
    let mut cnt = vec![0u32; bins];
    for n in 0..n_frames {
        let s = n * hop;
        if s < wb || s + wf > x.len() {
            continue;
        }
        let fwd = mean_square(&prefix, s, s + wf);
        let bwd = mean_square(&prefix, s - wb, s);
        let r = ((fwd + eps) / (bwd + eps)).ln().max(0.0);
        let t = s as f64 / fs;
        let b = (((t / period).fract()) * bins as f64) as usize % bins;
        sum[b] += r;
        cnt[b] += 1;
    }
    let h: Vec<f64> = sum
        .iter()
        .zip(&cnt)
        .map(|(s, &c)| if c > 0 { s / c as f64 } else { 0.0 })
        .collect();
    let center = ((coarse / period) * bins as f64).round() as isize;
    let reach = ((0.025 / period) * bins as f64).round() as isize;
    let mut best = (center, f64::MIN);
    for d in -reach..=reach {
        let i = (center + d).rem_euclid(bins as isize);
        if h[i as usize] > best.1 {
            best = (center + d, h[i as usize]);
        }
    }
    if best.1 <= 0.0 {
        return coarse.rem_euclid(period);
    }
    // Bin b covers [b, b+1) hops; the frame time is its left edge.
    (best.0 as f64 / bins as f64 * period).rem_euclid(period)
}

/// Bar position (1..=4) of the beat at `phase` (the first beat of the grid).
fn downbeat_position(
    x: &[f32],
    fs: f64,
    onsets: &Onsets,
    period: f64,
    phase: f64,
    duration: f64,
) -> u8 {
    let n_beats = ((duration - phase) / period).floor() as usize + 1;
    if n_beats < 8 {
        return 1;
    }
    let low_sig = {
        let mut y = x.to_vec();
        let lp = Biquad::lowpass(fs, 150.0, std::f64::consts::FRAC_1_SQRT_2);
        lp.process(&mut y);
        lp.process(&mut y);
        y
    };
    let p_all = energy_prefix(x);
    let p_low = energy_prefix(&low_sig);
    let fr = onsets.frame_rate;
    let db = |v: f64| 10.0 * (v + 1e-10).log10();
    let mut low_onset = Vec::with_capacity(n_beats);
    let mut feats = Vec::with_capacity(n_beats);
    for i in 0..n_beats {
        let t = phase + i as f64 * period;
        let c = (t * fr).round() as isize;
        let m = (c - 3..=c + 3)
            .filter(|&k| k >= 0 && (k as usize) < onsets.low.len())
            .map(|k| onsets.low[k as usize])
            .fold(0f32, f32::max);
        low_onset.push(m as f64);
        let a = (t * fs) as usize;
        let b = ((t + period) * fs) as usize;
        let e_all = mean_square(&p_all, a, b);
        let e_low = mean_square(&p_low, a, b);
        feats.push((db(e_all), db(e_low), e_all));
    }
    let max_e = feats.iter().map(|f| f.2).fold(0.0, f64::max);
    let novelty: Vec<f64> = (0..n_beats)
        .map(|i| {
            if i == 0 {
                0.0
            } else {
                (feats[i].0 - feats[i - 1].0).abs() + (feats[i].1 - feats[i - 1].1).abs()
            }
        })
        .collect();
    let mean = |v: &[f64]| (v.iter().sum::<f64>() / v.len() as f64).max(1e-12);
    let (m_on, m_nov) = (mean(&low_onset), mean(&novelty));
    let mut score = [0f64; 4];
    let mut count = [0u32; 4];
    for i in 0..n_beats {
        score[i % 4] += low_onset[i] / m_on + 0.5 * novelty[i] / m_nov;
        count[i % 4] += 1;
    }
    for o in 0..4 {
        score[o] /= count[o].max(1) as f64;
    }
    // Prior: the first clearly audible beat is usually a downbeat.
    if let Some(first) = feats.iter().position(|f| f.2 > max_e * 0.01) {
        score[first % 4] += 0.15;
    }
    let (best, _) = argmax(&score);
    // Beat `best` is a downbeat, so beat 0 sits at position (0 - best) mod 4.
    ((4 - best % 4) % 4 + 1) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testsig::*;

    fn opts() -> AnalysisOptions {
        AnalysisOptions::default()
    }

    /// Asserts BPM within 0.05 and every click within `tol` of a grid beat.
    fn check(grid: &Beatgrid, bpm: f64, onsets: &[f64], tol: f64) {
        let got = grid.first_bpm().expect("grid should not be empty");
        assert!((got - bpm).abs() < 0.05, "bpm {got} vs {bpm}");
        for &t in onsets {
            let i = grid.beat_index_at(t + 0.1).unwrap();
            let d = (grid.beats[i].time - t)
                .abs()
                .min(grid.beats.get(i + 1).map_or(1.0, |b| (b.time - t).abs()));
            let d = d.min(if i > 0 {
                (grid.beats[i - 1].time - t).abs()
            } else {
                1.0
            });
            assert!(
                d < tol,
                "beat at {t:.4}s off by {:.2} ms (bpm {got})",
                d * 1000.0
            );
        }
    }

    #[test]
    fn kick_tracks_at_various_tempos() {
        for (bpm, offset) in [
            (120.0, 0.137),
            (124.5, 0.31),
            (128.0, 0.05),
            (140.0, 0.4),
            (174.0, 0.222),
        ] {
            let (x, onsets) = kick_track(44_100, bpm, offset, 40.0, true);
            let g = detect_beatgrid(&x, 44_100, &opts());
            check(&g, bpm, &onsets, 0.005);
        }
    }

    #[test]
    fn click_tracks_at_48k() {
        for (bpm, offset) in [(126.0, 0.2), (93.0, 0.01), (150.37, 0.6)] {
            let (x, onsets) = click_track(48_000, bpm, offset, 30.0);
            let g = detect_beatgrid(&x, 48_000, &opts());
            check(&g, bpm, &onsets, 0.005);
        }
    }

    #[test]
    fn half_and_double_time_fold_into_range() {
        // 65 BPM clicks → 130; 70 → 140.
        for (play, expect) in [(65.0, 130.0), (70.0, 140.0)] {
            let (x, _) = click_track(44_100, play, 0.1, 40.0);
            let g = detect_beatgrid(&x, 44_100, &opts());
            assert!(
                (g.first_bpm().unwrap() - expect).abs() < 0.05,
                "{play}: {:?}",
                g.first_bpm()
            );
        }
        // 240 BPM clicks (16ths at 120 would be denser) → 120.
        let (x, _) = click_track(44_100, 240.0, 0.1, 30.0);
        let g = detect_beatgrid(&x, 44_100, &opts());
        assert!(
            (g.first_bpm().unwrap() - 120.0).abs() < 0.05,
            "{:?}",
            g.first_bpm()
        );
        // Kick on 1 and 3 only at 87 BPM half-time feel with even hats → 174.
        let (x, _) = kick_track(44_100, 174.0, 0.1, 30.0, false);
        let g = detect_beatgrid(&x, 44_100, &opts());
        assert!(
            (g.first_bpm().unwrap() - 174.0).abs() < 0.05,
            "{:?}",
            g.first_bpm()
        );
    }

    #[test]
    fn house_pattern_with_hats_and_bass() {
        let (x, kicks) = house_track(44_100, 128.0, 0.21, 60.0);
        let g = detect_beatgrid(&x, 44_100, &opts());
        check(&g, 128.0, &kicks, 0.005);
    }

    #[test]
    fn accented_downbeats_are_found() {
        // Kick track with louder kick on beat 1 of every bar starting at the
        // third click (so beat 0 is bar position 3).
        let bpm = 125.0;
        let (x, onsets) = accented_track(44_100, bpm, 0.3, 30.0, 2);
        let g = detect_beatgrid(&x, 44_100, &opts());
        check(&g, bpm, &onsets, 0.005);
        let i = g.beat_index_at(onsets[2] + 0.01).unwrap();
        assert_eq!(g.beats[i].bar_position, 1);
    }

    #[test]
    fn long_track_has_no_drift() {
        // Six minutes at a non-round tempo; grid must stay within 5 ms.
        let (x, onsets) = click_track(22_050, 127.83, 0.123, 360.0);
        let g = detect_beatgrid(&x, 22_050, &opts());
        let probe: Vec<f64> = onsets
            .iter()
            .step_by(37)
            .copied()
            .chain(onsets.last().copied())
            .collect();
        check(&g, 127.83, &probe, 0.005);
    }

    #[test]
    fn silence_and_tone_give_empty_grid() {
        assert!(detect_beatgrid(&vec![0.0; 44_100 * 10], 44_100, &opts()).is_empty());
        assert!(detect_beatgrid(&[], 44_100, &opts()).is_empty());
        let tone: Vec<f32> = (0..44_100 * 10)
            .map(|i| (i as f32 * 0.05).sin() * 0.5)
            .collect();
        assert!(detect_beatgrid(&tone, 44_100, &opts()).is_empty());
    }

    #[test]
    fn lite_mode_is_close() {
        let lite = AnalysisOptions {
            lite: true,
            ..AnalysisOptions::default()
        };
        let (x, onsets) = kick_track(44_100, 128.0, 0.05, 40.0, true);
        let g = detect_beatgrid(&x, 44_100, &lite);
        check(&g, 128.0, &onsets, 0.008);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_bpm(127.9996, 360.0), 128.0);
        assert_eq!(snap_bpm(170.0028, 360.0), 170.0);
        assert!((snap_bpm(150.37, 30.0) - 150.37).abs() < 1e-9);
        assert_eq!(snap_bpm(127.8301, 360.0), 127.83);
        assert!((snap_bpm(127.8337, 360.0) - 127.8337).abs() < 1e-9);
    }
}
