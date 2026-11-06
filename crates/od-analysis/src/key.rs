//! Musical key detection.
//!
//! 1. A long-window STFT (≈ 0.7 s at the analysis rate) is mapped onto a
//!    pitch-class profile (chromagram) between [`MIN_FREQ`] and [`MAX_FREQ`],
//!    after estimating the global tuning offset from spectral peaks.
//! 2. Frame chroma vectors are normalised (every non-silent frame counts
//!    equally) and summed over the track.
//! 3. The summed profile is correlated (Pearson) with 24 rotated key
//!    templates — an average of the Krumhansl–Kessler and Temperley profiles —
//!    and the best match wins.

use crate::dsp::{decimate, decimation_factor, stft_map};
use od_core::{Mode, MusicalKey};
use std::f64::consts::PI;

/// Lowest frequency considered for chroma (≈ A1).
pub const MIN_FREQ: f64 = 55.0;
/// Highest frequency considered for chroma (≈ A6).
pub const MAX_FREQ: f64 = 1760.0;

const KK_MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const KK_MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
const TEMPERLEY_MAJOR: [f64; 12] = [
    0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400,
];
const TEMPERLEY_MINOR: [f64; 12] = [
    0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330,
];

fn blend(a: &[f64; 12], b: &[f64; 12]) -> [f64; 12] {
    let sa: f64 = a.iter().sum();
    let sb: f64 = b.iter().sum();
    std::array::from_fn(|i| a[i] / sa + b[i] / sb)
}

/// Settings of the chroma front end.
#[derive(Clone, Copy, Debug)]
struct ChromaParams {
    n_fft: usize,
    hop: usize,
}

/// Detects the key of a mono signal. Returns `None` for silence or signals
/// without tonal content.
pub fn detect_key(samples: &[f32], sample_rate: u32) -> Option<MusicalKey> {
    let factor = decimation_factor(sample_rate, 11_025);
    let x = decimate(samples, factor);
    detect_key_at(&x, sample_rate as f64 / factor as f64, false)
}

/// Key detection on a signal already at the analysis rate (≈ 8–16 kHz).
pub(crate) fn detect_key_at(x: &[f32], fs: f64, lite: bool) -> Option<MusicalKey> {
    let chroma = chromagram_sum(x, fs, lite)?;
    best_key(&chroma)
}

/// Summed, frame-normalised chroma vector (index 0 = C).
pub(crate) fn chromagram_sum(x: &[f32], fs: f64, lite: bool) -> Option<[f64; 12]> {
    if x.is_empty() || fs <= 0.0 {
        return None;
    }
    // ~0.7 s window for ~1.3 Hz bins at 11 kHz.
    let n_fft = if lite {
        4096
    } else {
        (fs * 0.74).round() as usize
    }
    .next_power_of_two();
    let p = ChromaParams {
        n_fft,
        hop: if lite { n_fft } else { n_fft / 2 },
    };
    let bin_hz = fs / p.n_fft as f64;
    let lo = (MIN_FREQ / bin_hz).ceil() as usize;
    let hi = ((MAX_FREQ.min(fs * 0.45)) / bin_hz).floor() as usize;
    if hi <= lo + 2 {
        return None;
    }
    // Spectral frames restricted to the pitch range (power spectrum).
    let frames: Vec<Vec<f32>> = stft_map(x, p.n_fft, p.hop, |_, m| {
        m[lo..=hi].iter().map(|v| v * v).collect()
    });
    let total: f64 = frames
        .iter()
        .flat_map(|f| f.iter())
        .map(|&v| v as f64)
        .sum();
    if total <= 1e-9 {
        return None;
    }

    // Tuning: circular mean of the semitone fraction of spectral peaks.
    let midi = |k: usize| 69.0 + 12.0 * ((k + lo) as f64 * bin_hz / 440.0).log2();
    let (mut re, mut im) = (0.0, 0.0);
    for f in &frames {
        for k in 1..f.len() - 1 {
            if f[k] > f[k - 1] && f[k] >= f[k + 1] {
                let ang = 2.0 * PI * midi(k);
                re += f[k] as f64 * ang.cos();
                im += f[k] as f64 * ang.sin();
            }
        }
    }
    let tuning = if re == 0.0 && im == 0.0 {
        0.0
    } else {
        im.atan2(re) / (2.0 * PI)
    };

    // Bin → (pitch class, weight) map with a raised-cosine semitone kernel.
    let map: Vec<(usize, f64)> = (0..=hi - lo)
        .map(|k| {
            let m = midi(k) - tuning;
            let r = m.round();
            let d = m - r;
            let w = (PI * d).cos().powi(2);
            ((r as i64).rem_euclid(12) as usize, w)
        })
        .collect();

    let mut sum = [0f64; 12];
    let frame_energy: Vec<f64> = frames
        .iter()
        .map(|f| f.iter().map(|&v| v as f64).sum())
        .collect();
    let max_e = frame_energy.iter().cloned().fold(0.0, f64::max);
    for (f, &e) in frames.iter().zip(&frame_energy) {
        if e < max_e * 1e-4 {
            continue;
        }
        let mut c = [0f64; 12];
        for (v, &(pc, w)) in f.iter().zip(&map) {
            c[pc] += (*v as f64).sqrt() * w;
        }
        let norm = c.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm > 0.0 {
            for i in 0..12 {
                sum[i] += c[i] / norm;
            }
        }
    }
    if sum.iter().all(|&v| v == 0.0) {
        None
    } else {
        Some(sum)
    }
}

fn pearson(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let ma = a.iter().sum::<f64>() / 12.0;
    let mb = b.iter().sum::<f64>() / 12.0;
    let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        num += (a[i] - ma) * (b[i] - mb);
        da += (a[i] - ma).powi(2);
        db += (b[i] - mb).powi(2);
    }
    if da <= 0.0 || db <= 0.0 {
        0.0
    } else {
        num / (da * db).sqrt()
    }
}

/// Picks the key whose rotated template correlates best with `chroma`.
/// Returns `None` if the profile is flat (no tonal centre).
pub fn best_key(chroma: &[f64; 12]) -> Option<MusicalKey> {
    let major = blend(&KK_MAJOR, &TEMPERLEY_MAJOR);
    let minor = blend(&KK_MINOR, &TEMPERLEY_MINOR);
    let mut best: Option<(f64, MusicalKey)> = None;
    for tonic in 0..12u8 {
        for (mode, prof) in [(Mode::Major, &major), (Mode::Minor, &minor)] {
            let rotated: [f64; 12] = std::array::from_fn(|i| prof[(i + 12 - tonic as usize) % 12]);
            let r = pearson(chroma, &rotated);
            if best.is_none_or(|(b, _)| r > b) {
                best = Some((r, MusicalKey::new(tonic, mode)));
            }
        }
    }
    best.filter(|(r, _)| *r > 0.2).map(|(_, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: u32 = 22_050;

    fn midi_hz(m: f64) -> f64 {
        440.0 * 2f64.powf((m - 69.0) / 12.0)
    }

    /// Renders chords (MIDI note lists), each lasting `secs`, with decaying
    /// harmonic partials.
    fn render(chords: &[&[i32]], secs: f64, repeats: usize) -> Vec<f32> {
        let n = (FS as f64 * secs) as usize;
        let mut out = Vec::new();
        for _ in 0..repeats {
            for chord in chords {
                for i in 0..n {
                    let t = i as f64 / FS as f64;
                    let env = (-t * 1.5).exp() * 0.8 + 0.2;
                    let mut v = 0.0;
                    for &m in chord.iter() {
                        let f = midi_hz(m as f64);
                        for h in 1..=4 {
                            v += (2.0 * PI * f * h as f64 * t).sin() / h as f64;
                        }
                    }
                    out.push((v * env * 0.1) as f32);
                }
            }
        }
        out
    }

    fn key(s: &str) -> MusicalKey {
        MusicalKey::parse(s).unwrap()
    }

    #[test]
    fn a_minor_progression() {
        // i – iv – V – i in A minor (with bass notes).
        let am: &[i32] = &[45, 57, 60, 64];
        let dm: &[i32] = &[50, 57, 62, 65];
        let e: &[i32] = &[40, 56, 59, 64];
        let x = render(&[am, dm, e, am], 1.5, 3);
        assert_eq!(detect_key(&x, FS), Some(key("Am")));
    }

    #[test]
    fn c_major_progression() {
        let c: &[i32] = &[48, 60, 64, 67];
        let f: &[i32] = &[41, 60, 65, 69];
        let g: &[i32] = &[43, 59, 62, 67];
        let x = render(&[c, f, g, c], 1.5, 3);
        assert_eq!(detect_key(&x, FS), Some(key("C")));
    }

    #[test]
    fn f_sharp_minor_progression() {
        let fsm: &[i32] = &[42, 54, 57, 61];
        let bm: &[i32] = &[47, 54, 59, 62];
        let cs: &[i32] = &[49, 56, 61, 65];
        let x = render(&[fsm, bm, cs, fsm], 1.5, 3);
        assert_eq!(detect_key(&x, FS), Some(key("F#m")));
    }

    #[test]
    fn scales() {
        // Eb major scale up and down with tonic chord drone.
        let scale = [63, 65, 67, 68, 70, 72, 74, 75, 74, 72, 70, 68, 67, 65, 63];
        let notes: Vec<Vec<i32>> = scale.iter().map(|&m| vec![51, m]).collect();
        let refs: Vec<&[i32]> = notes.iter().map(|v| v.as_slice()).collect();
        let x = render(&refs, 0.4, 2);
        assert_eq!(detect_key(&x, FS), Some(key("Eb")));
        // D harmonic minor.
        let scale = [62, 64, 65, 67, 69, 70, 73, 74, 73, 70, 69, 67, 65, 64, 62];
        let notes: Vec<Vec<i32>> = scale.iter().map(|&m| vec![50, m]).collect();
        let refs: Vec<&[i32]> = notes.iter().map(|v| v.as_slice()).collect();
        let x = render(&refs, 0.4, 2);
        assert_eq!(detect_key(&x, FS), Some(key("Dm")));
    }

    #[test]
    fn detuned_by_a_third_of_a_semitone() {
        // Same A minor progression played 30 cents sharp.
        let am: &[i32] = &[45, 57, 60, 64];
        let dm: &[i32] = &[50, 57, 62, 65];
        let e: &[i32] = &[40, 56, 59, 64];
        let x = render(&[am, dm, e, am], 1.5, 3);
        // Resampling-free detune: stretch time by reading faster.
        let ratio = 2f64.powf(0.3 / 12.0);
        let y: Vec<f32> = (0..(x.len() as f64 / ratio) as usize)
            .map(|i| x[((i as f64) * ratio) as usize])
            .collect();
        assert_eq!(detect_key(&y, FS), Some(key("Am")));
    }

    #[test]
    fn silence_has_no_key() {
        assert_eq!(detect_key(&vec![0.0; 44_100], 44_100), None);
        assert_eq!(detect_key(&[], 44_100), None);
    }
}
