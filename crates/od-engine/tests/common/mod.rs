//! Shared helpers for offline-render tests.
#![allow(dead_code)]

use std::sync::Arc;

use od_core::{Beatgrid, TrackId};
use od_decode::DecodedAudio;
use od_engine::*;

pub const SR: u32 = 48_000;
pub const BLOCK: usize = 128;
/// Scale of the position-encoding ramp: sample value × RAMP_SCALE = frame index.
pub const RAMP_SCALE: f32 = 1_048_576.0;

pub fn track_from(id: i64, samples: Vec<f32>, bpm: f64, first_beat: f64) -> Arc<TrackData> {
    let frames = samples.len() / 2;
    let dur = frames as f64 / SR as f64;
    let grid = if bpm > 0.0 {
        Beatgrid::constant(bpm, first_beat, 1, dur)
    } else {
        Beatgrid::default()
    };
    TrackData::new(
        TrackId(id),
        DecodedAudio {
            sample_rate: SR,
            channels: 2,
            samples,
        },
        SR,
        grid,
        &[],
        0.0,
    )
}

/// Left channel = frame index / RAMP_SCALE (exactly representable), right = same.
pub fn ramp_track(id: i64, seconds: f64, bpm: f64) -> Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let s = (0..n).flat_map(|i| {
        let v = i as f32 / RAMP_SCALE;
        [v, v]
    });
    track_from(id, s.collect(), bpm, 0.0)
}

pub fn sine_track(id: i64, seconds: f64, freq: f64, amp: f32, bpm: f64) -> Arc<TrackData> {
    let n = (seconds * SR as f64) as usize;
    let s = (0..n).flat_map(|i| {
        let v = (std::f64::consts::TAU * freq * i as f64 / SR as f64).sin() as f32 * amp;
        [v, v]
    });
    track_from(id, s.collect(), bpm, 0.0)
}

pub struct Rig {
    pub engine: Engine,
    pub proc: EngineProcessor,
    pub master: Vec<f32>,
    pub phones: Vec<f32>,
}

impl Rig {
    pub fn new(decks: usize) -> Self {
        Self::with(EngineConfig {
            sample_rate: SR,
            max_block: 512,
            decks,
            profile: Profile::Full,
        })
    }

    pub fn with(config: EngineConfig) -> Self {
        let (mut engine, proc) = Engine::new(config);
        // Neutral mixer for tests: limiter off so samples pass bit-exact.
        engine
            .send(Command::Mixer(MixerCommand::SetLimiter(false)))
            .unwrap();
        Self {
            engine,
            proc,
            master: vec![0.0; 8192 * 2],
            phones: vec![0.0; 8192 * 2],
        }
    }

    pub fn send(&mut self, c: Command) {
        self.engine.send(c).unwrap();
    }

    pub fn deck(&mut self, d: usize, c: DeckCommand) {
        self.engine.deck(d, c).unwrap();
    }

    /// Renders one block and returns the master output of that block.
    pub fn block(&mut self, frames: usize) -> &[f32] {
        let mut out = Outputs {
            master: &mut self.master[..frames * 2],
            headphones: Some(&mut self.phones[..frames * 2]),
        };
        self.proc.process(frames, &mut out);
        &self.master[..frames * 2]
    }

    /// Renders `blocks` blocks of `BLOCK` frames, collecting master output.
    pub fn render(&mut self, blocks: usize) -> Vec<f32> {
        let mut all = Vec::with_capacity(blocks * BLOCK * 2);
        for _ in 0..blocks {
            let b = self.block(BLOCK).to_vec();
            all.extend_from_slice(&b);
        }
        all
    }

    pub fn state(&mut self) -> EngineState {
        *self.engine.state()
    }
}

/// Frequency estimate via zero crossings of the left channel (with linear
/// interpolation of the crossing instants).
pub fn estimate_freq(interleaved: &[f32]) -> f64 {
    let l: Vec<f32> = interleaved
        .as_chunks::<2>()
        .0
        .iter()
        .map(|f| f[0])
        .collect();
    let mut first = None;
    let mut last = 0.0;
    let mut count = 0;
    for i in 1..l.len() {
        if l[i - 1] < 0.0 && l[i] >= 0.0 {
            let t = (i - 1) as f64 + (-l[i - 1] / (l[i] - l[i - 1])) as f64;
            if first.is_none() {
                first = Some(t);
            } else {
                count += 1;
            }
            last = t;
        }
    }
    let first = first.unwrap_or(0.0);
    if count == 0 {
        0.0
    } else {
        count as f64 * SR as f64 / (last - first)
    }
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}
