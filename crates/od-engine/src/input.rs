//! Microphone / aux input channel (SPEC §10.2): an external stereo or mono
//! source mixed into the master after the deck channels, with level, on/off,
//! headphone cue, meter and talkover (the music ducks while the mic is in
//! use). It sits before Beat FX, booth, master level and limiter, so it is
//! heard on every output and recorded.
//!
//! The capture side runs at the engine sample rate and delivers interleaved
//! samples through a lock-free ring; the app checks the rates match.

use crate::command::InputCommand;
use crate::dsp::meter::Meter;
use crate::dsp::smooth::Smoothed;
use crate::mixer::level_gain;
use crate::rt::{Garbage, RtCtx};
use crate::state::InputState;

/// Music level while talkover is engaged, by default −12 dB.
const TALKOVER_DEFAULT_DB: f32 = -12.0;
/// Mic level (linear peak) that engages talkover.
const TALKOVER_THRESHOLD: f32 = 0.03;
/// Talkover hold after the mic falls silent, seconds.
const TALKOVER_HOLD: f32 = 0.4;
/// Buffered input beyond this many blocks is skipped (keeps latency low).
const MAX_BACKLOG_BLOCKS: usize = 3;

pub(crate) struct AuxInput {
    source: Option<(rtrb::Consumer<f32>, u16)>,
    on: bool,
    cue: bool,
    level: f32,
    gain: Smoothed,
    talkover: bool,
    talkover_db: f32,
    duck: Smoothed,
    hold: f32,
    meter: Meter,
    underruns: u64,
    sr: f32,
}

impl AuxInput {
    pub fn new(sample_rate: u32) -> Self {
        let ramp = (sample_rate as f32 * 0.01) as u32;
        Self {
            source: None,
            on: false,
            cue: false,
            level: 0.8,
            gain: Smoothed::new(0.0, ramp),
            talkover: false,
            talkover_db: TALKOVER_DEFAULT_DB,
            duck: Smoothed::new(1.0, (sample_rate as f32 * 0.05) as u32),
            hold: 0.0,
            meter: Meter::new(sample_rate),
            underruns: 0,
            sr: sample_rate as f32,
        }
    }

    pub fn handle(&mut self, cmd: InputCommand, ctx: &mut RtCtx<'_>) {
        match cmd {
            InputCommand::Attach(consumer, channels) => {
                if let Some((old, _)) = self.source.take() {
                    ctx.dispose(Garbage::Input(old));
                }
                self.source = Some((consumer, channels.max(1)));
            }
            InputCommand::Detach => {
                if let Some((old, _)) = self.source.take() {
                    ctx.dispose(Garbage::Input(old));
                }
            }
            InputCommand::SetOn(on) => self.on = on,
            InputCommand::SetLevel(l) => self.level = l.clamp(0.0, 1.5),
            InputCommand::SetCue(c) => self.cue = c,
            InputCommand::SetTalkover(on) => self.talkover = on,
            InputCommand::SetTalkoverDepth(db) => self.talkover_db = db.clamp(-40.0, 0.0),
        }
    }

    /// Reads `buf.len() / 2` frames of input into `buf` (stereo, gain
    /// applied), mixes them into `master` (ducked first when talkover is
    /// engaged) and `cue`.
    pub fn process(&mut self, buf: &mut [f32], master: &mut [f32], cue: &mut [f32]) {
        let frames = buf.len() / 2;
        buf.fill(0.0);
        let Some((ring, channels)) = self.source.as_mut() else {
            self.meter.process(buf);
            return;
        };
        let ch = *channels as usize;
        // Drop backlog beyond a few blocks.
        let backlog = ring.slots();
        let keep = frames * ch * MAX_BACKLOG_BLOCKS;
        if backlog > keep {
            let skip = (backlog - keep) / ch * ch;
            if let Ok(chunk) = ring.read_chunk(skip) {
                chunk.commit_all();
            }
        }
        let avail = (ring.slots() / ch).min(frames);
        if avail < frames {
            self.underruns += 1;
        }
        if let Ok(chunk) = ring.read_chunk(avail * ch) {
            let (a, b) = chunk.as_slices();
            for f in 0..avail {
                let at = |i: usize| if i < a.len() { a[i] } else { b[i - a.len()] };
                let l = at(f * ch);
                let r = if ch > 1 { at(f * ch + 1) } else { l };
                buf[f * 2] = l;
                buf[f * 2 + 1] = r;
            }
            chunk.commit_all();
        }
        self.gain.set(if self.on { level_gain(self.level) } else { 0.0 });
        let mut peak = 0.0f32;
        for f in buf.as_chunks_mut::<2>().0 {
            let g = self.gain.next();
            f[0] *= g;
            f[1] *= g;
            peak = peak.max(f[0].abs()).max(f[1].abs());
        }
        self.meter.process(buf);
        // Talkover: duck the music while the mic carries signal.
        let block_s = frames as f32 / self.sr;
        if self.on && self.talkover && peak > TALKOVER_THRESHOLD {
            self.hold = TALKOVER_HOLD;
        } else {
            self.hold = (self.hold - block_s).max(0.0);
        }
        let engaged = self.on && self.talkover && self.hold > 0.0;
        self.duck.set(if engaged { 10f32.powf(self.talkover_db / 20.0) } else { 1.0 });
        for (m, s) in master.as_chunks_mut::<2>().0.iter_mut().zip(buf.chunks_exact(2)) {
            let d = self.duck.next();
            m[0] = m[0] * d + s[0];
            m[1] = m[1] * d + s[1];
        }
        if self.cue {
            for (c, s) in cue.iter_mut().zip(buf.iter()) {
                *c += *s;
            }
        }
    }

    pub fn state(&self) -> InputState {
        InputState {
            attached: self.source.is_some(),
            on: self.on,
            level: self.level,
            cue: self.cue,
            talkover: self.talkover,
            talkover_db: self.talkover_db,
            ducking: self.hold > 0.0 && self.on && self.talkover,
            meter: self.meter.reading(),
            underruns: self.underruns,
        }
    }
}

