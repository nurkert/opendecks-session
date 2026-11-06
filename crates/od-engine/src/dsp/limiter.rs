//! Master bus protection: a fast feed-forward peak limiter followed by a soft
//! clipper, so the output never exceeds ±1.0.

pub struct Limiter {
    threshold: f32,
    attack: f32,
    release: f32,
    env: f32,
    gain: f32,
    min_gain_block: f32,
}

impl Limiter {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        Self {
            threshold: 0.89, // −1 dBFS
            attack: 1.0 - (-1.0 / (sr * 0.0005)).exp(),
            release: 1.0 - (-1.0 / (sr * 0.15)).exp(),
            env: 0.0,
            gain: 1.0,
            min_gain_block: 1.0,
        }
    }

    /// Processes interleaved stereo in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        self.min_gain_block = 1.0;
        for f in buf.as_chunks_mut::<2>().0 {
            let peak = f[0].abs().max(f[1].abs());
            let coeff = if peak > self.env {
                self.attack
            } else {
                self.release
            };
            self.env += (peak - self.env) * coeff;
            let target = if self.env > self.threshold {
                self.threshold / self.env
            } else {
                1.0
            };
            // Gain follows target with the same ballistics (attack when reducing).
            let gc = if target < self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain += (target - self.gain) * gc;
            self.min_gain_block = self.min_gain_block.min(self.gain);
            f[0] = super::soft_clip(f[0] * self.gain);
            f[1] = super::soft_clip(f[1] * self.gain);
        }
    }

    /// Largest gain reduction during the last block in dB (≥ 0).
    pub fn gain_reduction_db(&self) -> f32 {
        -super::gain_to_db(self.min_gain_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_exceeds_full_scale() {
        let mut l = Limiter::new(48_000);
        let mut buf: Vec<f32> = (0..48_000)
            .flat_map(|i| {
                let v = 3.0 * (i as f32 * 0.05).sin();
                [v, v]
            })
            .collect();
        l.process(&mut buf);
        assert!(buf.iter().all(|v| v.abs() <= 1.0));
        assert!(l.gain_reduction_db() > 6.0);
    }
}
