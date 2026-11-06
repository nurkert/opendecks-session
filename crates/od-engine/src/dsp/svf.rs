//! Topology-preserving state variable filter (Cytomic / Simper), stereo.
//! Stable under fast cutoff modulation, used for sweeps and colour filters.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    g: f32,
    k: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    ic1: [f32; 2],
    ic2: [f32; 2],
}

/// Outputs of one SVF tick.
#[derive(Clone, Copy, Debug)]
pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    pub fn new(freq: f32, q: f32, sr: f32) -> Self {
        let mut s = Self::default();
        s.set(freq, q, sr);
        s
    }

    /// Updates cutoff (Hz) and resonance (Q, 0.5 = no resonance).
    pub fn set(&mut self, freq: f32, q: f32, sr: f32) {
        let f = freq.clamp(10.0, sr * 0.49);
        self.g = (PI * f / sr).tan();
        self.k = 1.0 / q.max(0.05);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    #[inline]
    pub fn tick(&mut self, ch: usize, v0: f32) -> SvfOut {
        let v3 = v0 - self.ic2[ch];
        let v1 = self.a1 * self.ic1[ch] + self.a2 * v3;
        let v2 = self.ic2[ch] + self.a2 * self.ic1[ch] + self.a3 * v3;
        self.ic1[ch] = 2.0 * v1 - self.ic1[ch];
        self.ic2[ch] = 2.0 * v2 - self.ic2[ch];
        SvfOut {
            low: v2,
            band: v1,
            high: v0 - self.k * v1 - v2,
        }
    }

    pub fn reset(&mut self) {
        self.ic1 = [0.0; 2];
        self.ic2 = [0.0; 2];
    }
}
