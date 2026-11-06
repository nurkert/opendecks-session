//! Peak and VU (RMS) metering.

/// Published meter values (linear, 0..1+).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeterReading {
    /// Peak with fast attack and ~20 dB/s fall-back.
    pub peak: f32,
    /// RMS with ~300 ms VU-like integration.
    pub rms: f32,
    /// Peak hold (falls after ~1.5 s).
    pub peak_hold: f32,
    /// Set once any sample exceeded 1.0; cleared by [`crate::Command::ResetClipIndicators`].
    pub clipped: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Meter {
    sr: f32,
    reading: MeterReading,
    mean_sq: f32,
    hold_frames: u32,
}

impl Meter {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sr: sample_rate as f32,
            reading: MeterReading::default(),
            mean_sq: 0.0,
            hold_frames: 0,
        }
    }

    /// Accounts one block of interleaved stereo.
    pub fn process(&mut self, buf: &[f32]) {
        let frames = buf.len() / 2;
        if frames == 0 {
            return;
        }
        let mut peak = 0f32;
        let mut sum_sq = 0f32;
        for &s in buf {
            let a = s.abs();
            peak = peak.max(a);
            sum_sq += s * s;
        }
        self.apply(peak, sum_sq / buf.len() as f32, frames);
    }

    /// Accounts one block given its peak and mean square directly.
    pub fn apply(&mut self, peak: f32, mean_sq: f32, frames: usize) {
        let dt = frames as f32 / self.sr;
        // VU: one-pole with 300 ms time constant on mean square.
        let a = (-dt / 0.3).exp();
        self.mean_sq = self.mean_sq * a + mean_sq * (1.0 - a);
        self.reading.rms = self.mean_sq.max(0.0).sqrt();
        // Peak: instant attack, 20 dB/s release.
        let fall = 10f32.powf(-20.0 * dt / 20.0);
        self.reading.peak = peak.max(self.reading.peak * fall);
        if peak >= self.reading.peak_hold {
            self.reading.peak_hold = peak;
            self.hold_frames = (self.sr * 1.5) as u32;
        } else if self.hold_frames > frames as u32 {
            self.hold_frames -= frames as u32;
        } else {
            self.reading.peak_hold = self.reading.peak;
        }
        if peak > 1.0 {
            self.reading.clipped = true;
        }
    }

    pub fn reading(&self) -> MeterReading {
        self.reading
    }

    pub fn reset_clip(&mut self) {
        self.reading.clipped = false;
    }
}
