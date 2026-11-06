//! Mixer channel strip and fader/crossfader curves.

use crate::colorfx::ColorFx;
use crate::command::{ChannelCommand, CrossfaderCurve, FaderCurve, XfAssign};
use crate::dsp::db_to_gain;
use crate::dsp::eq::Eq3;
use crate::dsp::meter::Meter;
use crate::dsp::smooth::Smoothed;
use crate::state::ChannelState;

/// Channel fader curve: fader position 0..=1 → linear gain.
pub fn fader_gain(x: f32, curve: FaderCurve) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match curve {
        FaderCurve::Linear => x,
        FaderCurve::Smooth => x * x,
        FaderCurve::Sharp => {
            let y = (x * 4.0).min(1.0);
            1.0 - (1.0 - y) * (1.0 - y)
        }
    }
}

/// Crossfader gains `(side A, side B)` for a crossfader position −1..=1.
pub fn crossfader_gains(x: f32, curve: CrossfaderCurve) -> (f32, f32) {
    let p = ((x.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
    match curve {
        CrossfaderCurve::ConstantPower => {
            let a = p * std::f32::consts::FRAC_PI_2;
            (a.cos(), a.sin())
        }
        CrossfaderCurve::Linear => (1.0 - p, p),
        CrossfaderCurve::Cut => {
            const CUT: f32 = 0.04;
            (((1.0 - p) / CUT).min(1.0), (p / CUT).min(1.0))
        }
    }
}

/// Level knob 0..=1 → gain (square law, 1.0 = 0 dB).
pub fn level_gain(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.5);
    x * x
}

pub(crate) struct Channel {
    pub trim_db: f32,
    pub auto_gain: bool,
    pub eq: Eq3,
    pub eq_knobs: [f32; 3],
    pub color: ColorFx,
    pub fader: f32,
    pub fader_curve: FaderCurve,
    pub xf: XfAssign,
    pub cue: bool,
    /// Pre-fader gain (trim + auto gain).
    pub pre_gain: Smoothed,
    /// Post-fader gain to the master bus (fader × crossfader).
    pub post_gain: Smoothed,
    pub meter: Meter,
    /// Smoothed RMS contributed to the master (audible-track logic).
    pub contribution: f32,
    pub gain_to_master: f32,
}

impl Channel {
    pub fn new(sample_rate: u32, seed: u32) -> Self {
        let ramp = (sample_rate as f32 * 0.01) as u32;
        Self {
            trim_db: 0.0,
            auto_gain: true,
            eq: Eq3::new(sample_rate),
            eq_knobs: [0.0; 3],
            color: ColorFx::new(sample_rate, seed),
            fader: 1.0,
            fader_curve: FaderCurve::default(),
            xf: XfAssign::Thru,
            cue: false,
            pre_gain: Smoothed::new(1.0, ramp),
            post_gain: Smoothed::new(1.0, ramp),
            meter: Meter::new(sample_rate),
            contribution: 0.0,
            gain_to_master: 1.0,
        }
    }

    pub fn apply(&mut self, cmd: ChannelCommand) {
        match cmd {
            ChannelCommand::SetTrim(db) => self.trim_db = db.clamp(-60.0, 12.0),
            ChannelCommand::SetAutoGain(on) => self.auto_gain = on,
            ChannelCommand::SetEq { band, value } => {
                if band < 3 {
                    self.eq_knobs[band] = value.clamp(-1.0, 1.0);
                    self.eq.set_band(band, value);
                }
            }
            ChannelCommand::SetEqMode(m) => self.eq.set_mode(m),
            ChannelCommand::SetColorFx(k) => self.color.set_kind(k),
            ChannelCommand::SetColorFxAmount(a) => self.color.set_amount(a),
            ChannelCommand::SetFader(v) => self.fader = v.clamp(0.0, 1.0),
            ChannelCommand::SetFaderCurve(c) => self.fader_curve = c,
            ChannelCommand::SetCrossfaderAssign(x) => self.xf = x,
            ChannelCommand::SetCue(on) => self.cue = on,
        }
    }

    /// Updates gain targets for this block.
    pub fn update_gains(&mut self, auto_gain_db: f32, xf: (f32, f32)) {
        let db = self.trim_db + if self.auto_gain { auto_gain_db } else { 0.0 };
        self.pre_gain.set(if self.trim_db <= -60.0 {
            0.0
        } else {
            db_to_gain(db)
        });
        let x = match self.xf {
            XfAssign::A => xf.0,
            XfAssign::B => xf.1,
            XfAssign::Thru => 1.0,
        };
        self.post_gain
            .set(fader_gain(self.fader, self.fader_curve) * x);
    }

    /// Trim → EQ → colour FX, in place; then meters (pre-fader).
    pub fn process_pre(&mut self, buf: &mut [f32], beat_frames: f32) {
        for f in buf.as_chunks_mut::<2>().0 {
            let g = self.pre_gain.next();
            f[0] *= g;
            f[1] *= g;
        }
        if !self.eq.is_flat() {
            self.eq.process(buf);
        }
        self.color.process(buf, beat_frames);
        self.meter.process(buf);
    }

    pub fn state(&self) -> ChannelState {
        ChannelState {
            meter: self.meter.reading(),
            fader: self.fader,
            trim_db: self.trim_db,
            auto_gain: self.auto_gain,
            eq: self.eq_knobs,
            color_fx: self.color.kind(),
            color_fx_amount: self.color.amount(),
            xf_assign: self.xf,
            cue: self.cue,
            gain_to_master: self.gain_to_master,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfader_curves() {
        let (a, b) = crossfader_gains(-1.0, CrossfaderCurve::ConstantPower);
        assert!((a - 1.0).abs() < 1e-6 && b.abs() < 1e-6);
        let (a, b) = crossfader_gains(0.0, CrossfaderCurve::ConstantPower);
        assert!((a * a + b * b - 1.0).abs() < 1e-5, "constant power");
        let (a, b) = crossfader_gains(0.0, CrossfaderCurve::Linear);
        assert!((a - 0.5).abs() < 1e-6 && (b - 0.5).abs() < 1e-6);
        let (a, b) = crossfader_gains(0.0, CrossfaderCurve::Cut);
        assert_eq!((a, b), (1.0, 1.0));
        let (a, b) = crossfader_gains(-0.95, CrossfaderCurve::Cut);
        assert_eq!(a, 1.0);
        assert!(b < 1.0);
        let (_, b) = crossfader_gains(-1.0, CrossfaderCurve::Cut);
        assert_eq!(b, 0.0);
    }

    #[test]
    fn fader_curves_monotonic() {
        for c in [FaderCurve::Linear, FaderCurve::Smooth, FaderCurve::Sharp] {
            let mut last = -1.0;
            for i in 0..=100 {
                let g = fader_gain(i as f32 / 100.0, c);
                assert!(g >= last);
                last = g;
            }
            assert_eq!(fader_gain(0.0, c), 0.0);
            assert!((fader_gain(1.0, c) - 1.0).abs() < 1e-6);
        }
        assert!(fader_gain(0.25, FaderCurve::Sharp) > 0.99);
    }
}
