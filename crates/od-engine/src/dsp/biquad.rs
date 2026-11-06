//! RBJ-cookbook biquads (transposed direct form II), stereo.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coeffs {
    pub const IDENTITY: Coeffs = Coeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn norm(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn w0(freq: f32, sr: f32) -> (f32, f32) {
        let w = 2.0 * PI * (freq / sr).clamp(1e-5, 0.49);
        (w.cos(), w.sin())
    }

    pub fn lowpass(freq: f32, q: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q);
        Self::norm(
            (1.0 - c) / 2.0,
            1.0 - c,
            (1.0 - c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn highpass(freq: f32, q: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q);
        Self::norm(
            (1.0 + c) / 2.0,
            -(1.0 + c),
            (1.0 + c) / 2.0,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn allpass(freq: f32, q: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let alpha = s / (2.0 * q);
        Self::norm(
            1.0 - alpha,
            -2.0 * c,
            1.0 + alpha,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn peaking(freq: f32, q: f32, gain_db: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = s / (2.0 * q);
        Self::norm(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    pub fn low_shelf(freq: f32, gain_db: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }

    pub fn high_shelf(freq: f32, gain_db: f32, sr: f32) -> Self {
        let (c, s) = Self::w0(freq, sr);
        let a = 10f32.powf(gain_db / 40.0);
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    }
}

/// A stereo biquad section.
#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub c: Coeffs,
    z1: [f32; 2],
    z2: [f32; 2],
}

impl Biquad {
    pub fn new(c: Coeffs) -> Self {
        Self {
            c,
            z1: [0.0; 2],
            z2: [0.0; 2],
        }
    }

    #[inline]
    pub fn process(&mut self, ch: usize, x: f32) -> f32 {
        let c = &self.c;
        let y = c.b0 * x + self.z1[ch];
        self.z1[ch] = c.b1 * x - c.a1 * y + self.z2[ch];
        self.z2[ch] = c.b2 * x - c.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = [0.0; 2];
        self.z2 = [0.0; 2];
    }
}
