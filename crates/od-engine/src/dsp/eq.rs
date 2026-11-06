//! Three-band DJ equaliser.
//!
//! * **Isolator** (default): the signal is split by Linkwitz–Riley 24 dB/oct
//!   crossovers at 250 Hz and 2.5 kHz into three bands that sum back flat.
//!   Each band gain runs from full kill (−inf dB, gain exactly 0) through
//!   unity to +6 dB.
//! * **Classic**: low shelf / peaking mid / high shelf, −26 dB … +6 dB.
//!
//! Gains are smoothed per sample, so knob moves never click.

use super::biquad::{Biquad, Coeffs};
use super::smooth::Smoothed;

pub const LOW_CROSSOVER_HZ: f32 = 250.0;
pub const HIGH_CROSSOVER_HZ: f32 = 2500.0;
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;
const CLASSIC_UPDATE: usize = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EqMode {
    #[default]
    Isolator,
    Classic,
}

/// Maps a knob value in −1..=1 (0 = unity) to a linear isolator band gain.
/// −1 is a full kill (exactly 0), +1 is +6 dB.
pub fn knob_to_gain(knob: f32) -> f32 {
    let k = knob.clamp(-1.0, 1.0);
    if k <= -0.999 {
        0.0
    } else if k < 0.0 {
        // Cubic taper: −0.5 ≈ −18 dB, smoothly approaching −inf.
        let x = 1.0 + k;
        x * x * x
    } else {
        10f32.powf(k * 6.0 / 20.0)
    }
}

/// Classic EQ: −1 → −26 dB, 0 → 0 dB, +1 → +6 dB.
fn knob_to_db_classic(knob: f32) -> f32 {
    let k = knob.clamp(-1.0, 1.0);
    if k < 0.0 { k * 26.0 } else { k * 6.0 }
}

#[derive(Clone, Copy, Debug)]
struct Lr4 {
    a: Biquad,
    b: Biquad,
}

impl Lr4 {
    fn new(c: Coeffs) -> Self {
        Self {
            a: Biquad::new(c),
            b: Biquad::new(c),
        }
    }
    #[inline]
    fn process(&mut self, ch: usize, x: f32) -> f32 {
        let y = self.a.process(ch, x);
        self.b.process(ch, y)
    }
}

pub struct Eq3 {
    sr: f32,
    mode: EqMode,
    knobs: [f32; 3],
    gains: [Smoothed; 3],
    // isolator
    lp1: Lr4,
    hp1: Lr4,
    lp2: Lr4,
    hp2: Lr4,
    ap2: Biquad,
    // classic
    classic: [Biquad; 3],
    classic_counter: usize,
}

impl Eq3 {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let ramp = (sr * 0.01) as u32; // 10 ms
        Self {
            sr,
            mode: EqMode::Isolator,
            knobs: [0.0; 3],
            gains: [Smoothed::new(1.0, ramp); 3],
            lp1: Lr4::new(Coeffs::lowpass(LOW_CROSSOVER_HZ, BUTTERWORTH_Q, sr)),
            hp1: Lr4::new(Coeffs::highpass(LOW_CROSSOVER_HZ, BUTTERWORTH_Q, sr)),
            lp2: Lr4::new(Coeffs::lowpass(HIGH_CROSSOVER_HZ, BUTTERWORTH_Q, sr)),
            hp2: Lr4::new(Coeffs::highpass(HIGH_CROSSOVER_HZ, BUTTERWORTH_Q, sr)),
            ap2: Biquad::new(Coeffs::allpass(HIGH_CROSSOVER_HZ, BUTTERWORTH_Q, sr)),
            classic: [Biquad::new(Coeffs::IDENTITY); 3],
            classic_counter: 0,
        }
    }

    pub fn set_mode(&mut self, mode: EqMode) {
        if mode != self.mode {
            self.mode = mode;
            for (i, k) in self.knobs.into_iter().enumerate() {
                self.gains[i].reset(self.target_for(k));
            }
            self.classic_counter = 0;
        }
    }

    fn target_for(&self, knob: f32) -> f32 {
        match self.mode {
            EqMode::Isolator => knob_to_gain(knob),
            EqMode::Classic => knob_to_db_classic(knob),
        }
    }

    /// Sets band `band` (0 = low, 1 = mid, 2 = high) to a knob value −1..=1.
    pub fn set_band(&mut self, band: usize, knob: f32) {
        if band < 3 {
            self.knobs[band] = knob;
            let t = self.target_for(knob);
            self.gains[band].set(t);
        }
    }

    pub fn is_flat(&self) -> bool {
        let unity = match self.mode {
            EqMode::Isolator => 1.0,
            EqMode::Classic => 0.0,
        };
        self.gains
            .iter()
            .all(|g| g.is_settled() && g.current() == unity)
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        match self.mode {
            EqMode::Isolator => {
                for f in buf.as_chunks_mut::<2>().0 {
                    let gl = self.gains[0].next();
                    let gm = self.gains[1].next();
                    let gh = self.gains[2].next();
                    for (ch, s) in f.iter_mut().enumerate() {
                        let x = *s;
                        let low = self.ap2.process(ch, self.lp1.process(ch, x));
                        let rest = self.hp1.process(ch, x);
                        let mid = self.lp2.process(ch, rest);
                        let high = self.hp2.process(ch, rest);
                        *s = gl * low + gm * mid + gh * high;
                    }
                }
            }
            EqMode::Classic => {
                for f in buf.as_chunks_mut::<2>().0 {
                    if self.classic_counter == 0 {
                        let (l, m, h) = (
                            self.gains[0].current(),
                            self.gains[1].current(),
                            self.gains[2].current(),
                        );
                        self.classic[0].c = Coeffs::low_shelf(100.0, l, self.sr);
                        self.classic[1].c = Coeffs::peaking(1000.0, 0.7, m, self.sr);
                        self.classic[2].c = Coeffs::high_shelf(8000.0, h, self.sr);
                    }
                    self.classic_counter = (self.classic_counter + 1) % CLASSIC_UPDATE;
                    for g in &mut self.gains {
                        g.next();
                    }
                    for (ch, s) in f.iter_mut().enumerate() {
                        let mut x = *s;
                        for b in &mut self.classic {
                            x = b.process(ch, x);
                        }
                        *s = x;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone_gain(eq: &mut Eq3, freq: f32, sr: u32) -> f32 {
        let n = sr as usize;
        let mut buf: Vec<f32> = (0..n)
            .flat_map(|i| {
                let v = (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin();
                [v, v]
            })
            .collect();
        eq.process(&mut buf);
        let tail = &buf[n..]; // second half
        let rms_out = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
        rms_out / std::f32::consts::FRAC_1_SQRT_2
    }

    #[test]
    fn isolator_flat_at_unity() {
        for f in [40.0, 250.0, 1000.0, 2500.0, 8000.0] {
            let mut eq = Eq3::new(48_000);
            let g = tone_gain(&mut eq, f, 48_000);
            assert!((20.0 * g.log10()).abs() < 0.1, "f={f} gain={g}");
        }
    }

    #[test]
    fn isolator_kills() {
        // Low and high kills reach > 40 dB one band-width away from the
        // crossover. The mid band is only a decade wide, so with 24 dB/oct
        // slopes the attenuation at its centre is ~35 dB.
        for (band, f, min_db) in [
            (0usize, 50.0f32, 40.0),
            (1, 790.0, 30.0),
            (2, 12_000.0, 40.0),
        ] {
            let mut eq = Eq3::new(48_000);
            eq.set_band(band, -1.0);
            let g = tone_gain(&mut eq, f, 48_000);
            let db = 20.0 * g.log10();
            assert!(db < -min_db, "band {band} at {f} Hz only {db} dB");
        }
    }
}
