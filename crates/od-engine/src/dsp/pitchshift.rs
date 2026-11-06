//! Real-time granular pitch shifter (two crossfaded read heads), used by the
//! Pitch / Helix Beat FX and the Pitch colour FX.
//!
//! Two read heads sweep through a delay window at `1 − ratio` frames per
//! frame (a "rotating tape head"); each fades in and out with a `sin²`
//! window, offset by half a window, so their gains always sum to 1. When a
//! head restarts its sweep, its start delay is shifted (by up to
//! [`SEARCH_S`]) to the offset whose recent waveform correlates best with
//! the other head (WSOLA-style), so grains join in phase: no comb
//! cancellation and no pitch error on tonal material. Latency is about half
//! the window. Everything is allocated in [`PitchShifter::new`].

use std::f32::consts::PI;

/// Grain start search range in seconds.
pub const SEARCH_S: f32 = 0.012;
/// Correlation length in frames.
const CORR_LEN: usize = 160;

pub struct PitchShifter {
    buf: Vec<[f32; 2]>,
    mono: Vec<f32>,
    write: usize,
    window: f32,
    search: usize,
    /// Phase of head 0 in 0..1; head 1 is half a window apart.
    phase: f32,
    /// Extra start delay of each head's current grain.
    off: [f32; 2],
}

impl PitchShifter {
    /// `window_frames`: grain window (≈ 40–60 ms works for music);
    /// `sample_rate` sizes the alignment search.
    pub fn new(window_frames: usize, sample_rate: f32) -> Self {
        let search = (sample_rate * SEARCH_S) as usize;
        let len = window_frames + search + CORR_LEN + 16;
        Self {
            buf: vec![[0.0; 2]; len],
            mono: vec![0.0; len],
            write: 0,
            window: window_frames as f32,
            search,
            phase: 0.0,
            off: [0.0; 2],
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|f| *f = [0.0; 2]);
        self.mono.iter_mut().for_each(|v| *v = 0.0);
        self.phase = 0.0;
        self.off = [0.0; 2];
    }

    /// Mono sample `d` frames ago (1 = newest).
    #[inline]
    fn mono_at(&self, d: usize) -> f32 {
        let len = self.mono.len();
        self.mono[(self.write + len - d % len) % len]
    }

    #[inline]
    fn read(&self, delay: f32) -> [f32; 2] {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let di = d.floor();
        let frac = d - di;
        let i0 = (self.write + len - di as usize) % len;
        let i1 = (i0 + len - 1) % len;
        let a = self.buf[i0];
        let b = self.buf[i1];
        [a[0] + (b[0] - a[0]) * frac, a[1] + (b[1] - a[1]) * frac]
    }

    /// Start offset for a grain at `base` delay that best continues the
    /// waveform of the other head (at delay `other`).
    fn best_offset(&self, base: f32, other: f32) -> f32 {
        let other = other.round().max(1.0) as usize;
        let base = base.round().max(1.0) as usize;
        let mut best = (0usize, f32::MIN);
        let mut off = 0;
        while off <= self.search {
            let (mut c, mut e) = (0.0f32, 1e-9f32);
            for k in 0..CORR_LEN {
                let a = self.mono_at(other + k);
                let b = self.mono_at(base + off + k);
                c += a * b;
                e += b * b;
            }
            let score = c / e.sqrt();
            if score > best.1 {
                best = (off, score);
            }
            off += 2;
        }
        best.0 as f32
    }

    /// Records a frame without producing output (keeps the history
    /// continuous while the effect is bypassed).
    #[inline]
    pub fn push(&mut self, x: [f32; 2]) {
        let len = self.buf.len();
        self.buf[self.write] = x;
        self.mono[self.write] = x[0] + x[1];
        self.write = (self.write + 1) % len;
    }

    /// Processes one stereo frame; `ratio` = output / input frequency.
    #[inline]
    pub fn process(&mut self, x: [f32; 2], ratio: f32) -> [f32; 2] {
        self.push(x);
        let w = self.window;
        let prev = [self.phase, (self.phase + 0.5).fract()];
        // Delay changes by (1 − ratio) frames per frame.
        self.phase += (1.0 - ratio) / w;
        self.phase -= self.phase.floor();
        let p = [self.phase, (self.phase + 0.5).fract()];
        for h in 0..2 {
            if (p[h] - prev[h]).abs() > 0.5 {
                // Head `h` restarts its sweep: align with the other head.
                let o = 1 - h;
                let other = 1.0 + p[o] * w + self.off[o];
                self.off[h] = self.best_offset(1.0 + p[h] * w, other);
            }
        }
        let a = self.read(1.0 + p[0] * w + self.off[0]);
        let b = self.read(1.0 + p[1] * w + self.off[1]);
        let g0 = (PI * p[0]).sin();
        let g0 = g0 * g0;
        let g1 = 1.0 - g0;
        [a[0] * g0 + b[0] * g1, a[1] * g0 + b[1] * g1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Strongest DFT bin (1 Hz steps) between 100 and 1200 Hz.
    fn freq(sig: &[f32], sr: f32) -> f32 {
        let mut best = (0.0, 0.0f64);
        for f in 100..1200 {
            let w = std::f64::consts::TAU * f as f64 / sr as f64;
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &x) in sig.iter().enumerate() {
                re += x as f64 * (w * i as f64).cos();
                im += x as f64 * (w * i as f64).sin();
            }
            let p = re * re + im * im;
            if p > best.1 {
                best = (f as f32, p);
            }
        }
        best.0
    }

    #[test]
    fn octave_up_and_down() {
        let sr = 48_000.0;
        for (ratio, want) in [(2.0f32, 880.0f32), (0.5, 220.0), (1.0, 440.0), (1.5, 660.0)] {
            let mut ps = PitchShifter::new(2048, sr);
            let out: Vec<f32> = (0..48_000)
                .map(|i| {
                    let v = (2.0 * PI * 440.0 * i as f32 / sr).sin() * 0.5;
                    ps.process([v, v], ratio)[0]
                })
                .collect();
            let f = freq(&out[4800..24_000], sr);
            assert!((f - want).abs() / want < 0.01, "ratio {ratio}: {f}");
            // Aligned grains do not cancel: level stays near the input's.
            let rms = (out[4800..].iter().map(|v| v * v).sum::<f32>() / 43_200.0).sqrt();
            assert!(rms > 0.3, "ratio {ratio}: rms {rms}");
        }
    }
}
