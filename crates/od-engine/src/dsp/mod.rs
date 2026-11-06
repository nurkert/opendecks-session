//! Small, allocation-free DSP building blocks used by the mixer and FX.
//!
//! Everything here allocates only in constructors (`new`), never in the
//! per-sample or per-block methods.

pub mod biquad;
pub mod delay;
pub mod eq;
pub mod limiter;
pub mod meter;
pub mod resample;
pub mod reverb;
pub mod smooth;
pub mod svf;

/// Converts decibels to a linear gain factor.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Converts a linear gain factor to decibels (−inf for 0).
#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    if g <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * g.log10()
    }
}

/// Small, fast pseudo-random generator for noise FX (xorshift32).
#[derive(Clone, Copy, Debug)]
pub struct Noise(u32);

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    /// Uniform white noise in −1..1.
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Gentle soft clipper: transparent below `knee`, smoothly saturating to ±1.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        let shaped = KNEE + (1.0 - KNEE) * over.tanh();
        shaped.copysign(x)
    }
}
