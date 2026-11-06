//! Shared DSP building blocks: biquad filters, channel mixdown, decimation
//! and a parallel short-time Fourier transform.
//!
//! Everything here is deterministic: parallel work is split into fixed-size
//! chunks whose results are combined in order, never via work-stealing float
//! reductions.

use rayon::prelude::*;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use std::f64::consts::PI;

/// Second-order IIR section (transposed direct form II, f64 state).
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Biquad {
    /// RBJ cookbook low-pass.
    pub fn lowpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0 / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - c) / 2.0 / a0,
            b1: (1.0 - c) / a0,
            b2: (1.0 - c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    /// RBJ cookbook high-pass.
    pub fn highpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0 / fs;
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    /// Filters `x` in place, starting from a zero state.
    pub fn process(&self, x: &mut [f32]) {
        let (mut z1, mut z2) = (0.0f64, 0.0f64);
        for v in x.iter_mut() {
            let input = *v as f64;
            let out = self.b0 * input + z1;
            z1 = self.b1 * input - self.a1 * out + z2;
            z2 = self.b2 * input - self.a2 * out;
            *v = out as f32;
        }
    }
}

/// Applies a cascade of biquads to a copy of `x`.
pub fn filtered(x: &[f32], chain: &[Biquad]) -> Vec<f32> {
    let mut y = x.to_vec();
    for bq in chain {
        bq.process(&mut y);
    }
    y
}

/// Mixes interleaved audio down to mono by averaging the channels.
pub fn mixdown(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => samples.to_vec(),
        n => {
            let inv = 1.0 / n as f32;
            samples
                .chunks_exact(n)
                .map(|f| f.iter().sum::<f32>() * inv)
                .collect()
        }
    }
}

/// Integer decimation factor that brings `sample_rate` close to (but not
/// below) `target` Hz.
pub fn decimation_factor(sample_rate: u32, target: u32) -> usize {
    ((sample_rate / target.max(1)) as usize).max(1)
}

/// Low-pass filters `x` with a linear-phase windowed-sinc FIR (no delay) and
/// keeps every `factor`-th sample.
pub fn decimate(x: &[f32], factor: usize) -> Vec<f32> {
    if factor <= 1 {
        return x.to_vec();
    }
    let half = 8 * factor;
    let cutoff = 0.45 / factor as f64; // cycles per input sample
    let taps: Vec<f32> = (0..=2 * half)
        .map(|k| {
            let d = k as f64 - half as f64;
            let sinc = if d == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * d).sin() / (PI * d)
            };
            let w = 0.42
                + 0.5 * (PI * d / half as f64).cos()
                + 0.08 * (2.0 * PI * d / half as f64).cos();
            sinc * w
        })
        .map(|v| v as f32)
        .collect();
    let norm: f32 = taps.iter().sum();
    let taps: Vec<f32> = taps.iter().map(|t| t / norm).collect();
    let n_out = x.len().div_ceil(factor);
    const CHUNK: usize = 1 << 14;
    let mut out = vec![0f32; n_out];
    out.par_chunks_mut(CHUNK)
        .enumerate()
        .for_each(|(ci, chunk)| {
            for (j, o) in chunk.iter_mut().enumerate() {
                let center = (ci * CHUNK + j) * factor;
                let start = center as isize - half as isize;
                let end = center + half + 1;
                *o = if start >= 0 && end <= x.len() {
                    dot(&x[start as usize..end], &taps)
                } else {
                    let mut acc = 0f32;
                    for (k, t) in taps.iter().enumerate() {
                        let i = start + k as isize;
                        if i >= 0 && (i as usize) < x.len() {
                            acc += x[i as usize] * t;
                        }
                    }
                    acc
                };
            }
        });
    out
}

/// Dot product with eight independent accumulators (vectorises well).
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut acc = [0f32; 8];
    let (ca, ra) = a.as_chunks::<8>();
    let (cb, rb) = b.as_chunks::<8>();
    for (x, y) in ca.iter().zip(cb) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    let mut s: f32 = acc.iter().sum();
    for (x, y) in ra.iter().zip(rb) {
        s += x * y;
    }
    s
}

/// Periodic Hann window.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()) as f32)
        .collect()
}

/// Number of STFT frames for a signal of `len` samples (centred framing).
pub fn frame_count(len: usize, hop: usize) -> usize {
    if len == 0 { 0 } else { len.div_ceil(hop) }
}

/// Parallel short-time Fourier transform with centred, Hann-windowed frames:
/// frame `i` is centred on sample `i * hop` (zero padded at the edges). For
/// every frame `f` receives the frame index and the magnitude spectrum
/// (`n_fft / 2 + 1` bins) and returns a per-frame result.
pub fn stft_map<T, F>(x: &[f32], n_fft: usize, hop: usize, f: F) -> Vec<T>
where
    T: Send,
    F: Fn(usize, &[f32]) -> T + Sync,
{
    stft_core(x, n_fft, hop, |_| {}, |i, _, cur| f(i, cur))
}

/// Like [`stft_map`], but first applies `transform` to every magnitude
/// spectrum in place and passes the transformed spectrum of the previous
/// frame too (`None` for frame 0). Used for spectral flux.
pub fn stft_map_pairs<T, P, F>(x: &[f32], n_fft: usize, hop: usize, transform: P, f: F) -> Vec<T>
where
    T: Send,
    P: Fn(&mut [f32]) + Sync,
    F: Fn(usize, Option<&[f32]>, &[f32]) -> T + Sync,
{
    stft_core(x, n_fft, hop, transform, f)
}

fn stft_core<T, P, F>(x: &[f32], n_fft: usize, hop: usize, transform: P, f: F) -> Vec<T>
where
    T: Send,
    P: Fn(&mut [f32]) + Sync,
    F: Fn(usize, Option<&[f32]>, &[f32]) -> T + Sync,
{
    let frames = frame_count(x.len(), hop);
    if frames == 0 || n_fft < 2 || hop == 0 {
        return Vec::new();
    }
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(n_fft);
    let window = hann(n_fft);
    const CHUNK: usize = 256;
    let chunks: Vec<Vec<T>> = (0..frames.div_ceil(CHUNK))
        .into_par_iter()
        .map(|c| {
            let mut input = fft.make_input_vec();
            let mut spec = fft.make_output_vec();
            let mut scratch = fft.make_scratch_vec();
            let mut cur = vec![0f32; n_fft / 2 + 1];
            let mut prev = vec![0f32; n_fft / 2 + 1];
            let mut spectrum = |i: usize, out: &mut [f32]| {
                let start = (i * hop) as isize - (n_fft / 2) as isize;
                for (k, v) in input.iter_mut().enumerate() {
                    let idx = start + k as isize;
                    *v = if idx >= 0 && (idx as usize) < x.len() {
                        x[idx as usize] * window[k]
                    } else {
                        0.0
                    };
                }
                // Buffer lengths come from the plan itself, so this cannot fail.
                if fft
                    .process_with_scratch(&mut input, &mut spec, &mut scratch)
                    .is_err()
                {
                    spec.iter_mut().for_each(|c| *c = Complex::new(0.0, 0.0));
                }
                for (m, s) in out.iter_mut().zip(spec.iter()) {
                    *m = s.norm();
                }
                transform(out);
            };
            let lo = c * CHUNK;
            let hi = (lo + CHUNK).min(frames);
            if lo > 0 {
                spectrum(lo - 1, &mut prev);
            }
            let mut res = Vec::with_capacity(hi - lo);
            for i in lo..hi {
                spectrum(i, &mut cur);
                res.push(f(i, (i > 0).then_some(prev.as_slice()), &cur));
                std::mem::swap(&mut prev, &mut cur);
            }
            res
        })
        .collect();
    chunks.into_iter().flatten().collect()
}

/// Peak absolute sample value.
pub fn peak_abs(x: &[f32]) -> f32 {
    x.iter().fold(0f32, |a, &b| a.max(b.abs()))
}

/// Prefix sums of squared samples (f64), length `x.len() + 1`, for O(1)
/// interval energies.
pub fn energy_prefix(x: &[f32]) -> Vec<f64> {
    let mut p = Vec::with_capacity(x.len() + 1);
    let mut acc = 0f64;
    p.push(0.0);
    for &v in x {
        acc += (v as f64) * (v as f64);
        p.push(acc);
    }
    p
}

/// Mean square of `x[a..b]` using a prefix table from [`energy_prefix`].
pub fn mean_square(prefix: &[f64], a: usize, b: usize) -> f64 {
    let n = prefix.len() - 1;
    let (a, b) = (a.min(n), b.min(n));
    if b <= a {
        0.0
    } else {
        (prefix[b] - prefix[a]) / (b - a) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(fs: f64, f: f64, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * f * i as f64 / fs).sin() as f32)
            .collect()
    }

    #[test]
    fn decimate_keeps_passband_and_rejects_alias() {
        let x = tone(44_100.0, 1000.0, 44_100);
        let y = decimate(&x, 4);
        assert_eq!(y.len(), 11_025);
        let p = peak_abs(&y[500..10_000]);
        assert!((p - 1.0).abs() < 0.02, "{p}");
        let x = tone(44_100.0, 9000.0, 44_100);
        let y = decimate(&x, 4);
        assert!(peak_abs(&y[500..10_000]) < 0.02);
    }

    #[test]
    fn decimate_has_no_delay() {
        let mut x = vec![0f32; 4000];
        x[2000] = 1.0;
        let y = decimate(&x, 4);
        let (imax, _) = y
            .iter()
            .enumerate()
            .fold((0, 0f32), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
        assert_eq!(imax, 500);
    }

    #[test]
    fn biquads_separate_bands() {
        let fs = 44_100.0;
        let lp = Biquad::lowpass(fs, 200.0, std::f64::consts::FRAC_1_SQRT_2);
        let hp = Biquad::highpass(fs, 200.0, std::f64::consts::FRAC_1_SQRT_2);
        let low = filtered(&tone(fs, 50.0, 44_100), &[lp, lp]);
        let high = filtered(&tone(fs, 5000.0, 44_100), &[lp, lp]);
        assert!(peak_abs(&low[20_000..]) > 0.9);
        assert!(peak_abs(&high[20_000..]) < 0.01);
        let high = filtered(&tone(fs, 5000.0, 44_100), &[hp, hp]);
        assert!(peak_abs(&high[20_000..]) > 0.9);
    }

    #[test]
    fn stft_finds_tone_bin() {
        let x = tone(8000.0, 1000.0, 8000);
        let bins = stft_map(&x, 256, 128, |_, m| {
            m.iter()
                .enumerate()
                .fold((0, 0f32), |a, (i, &v)| if v > a.1 { (i, v) } else { a })
                .0
        });
        assert_eq!(bins.len(), 63);
        assert_eq!(bins[30], 32);
    }
}
