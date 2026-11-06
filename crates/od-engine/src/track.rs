//! Audio data handed to the engine: whole decoded tracks and sampler samples.
//!
//! Both are prepared off the real-time thread (decoding, resampling to the
//! engine rate) and shared with the processor as `Arc`s. The processor never
//! drops the last reference: replaced data is sent back to the control side
//! (see [`crate::Engine::collect_garbage`]).

use od_core::{Beatgrid, Cue, CueKind, TrackId};
use od_decode::{DecodeError, DecodedAudio, Layout};
use std::path::Path;
use std::sync::Arc;

use crate::config::HOT_CUES;

/// A hot cue inside the engine, positions in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HotCue {
    pub start: f64,
    /// `Some` for a loop hot cue.
    pub loop_end: Option<f64>,
}

/// A fully decoded track, interleaved stereo `f32` at the engine rate.
#[derive(Debug)]
pub struct TrackData {
    pub id: TrackId,
    /// Always the engine sample rate.
    pub sample_rate: u32,
    /// Interleaved stereo samples.
    pub samples: Box<[f32]>,
    pub beatgrid: Beatgrid,
    /// Hot cues A–H as stored in the library.
    pub hot_cues: [Option<HotCue>; HOT_CUES],
    /// Memory cue (and memory loop) start positions in seconds, sorted.
    pub memory_cues: Box<[HotCue]>,
    /// Auto-gain in dB derived from loudness analysis (0 if unknown).
    pub auto_gain_db: f32,
}

impl TrackData {
    /// Builds track data from decoded audio, resampling to `engine_rate` if
    /// needed (slow; call off the real-time thread). The audio is mixed to
    /// stereo if it is not already.
    pub fn new(
        id: TrackId,
        audio: DecodedAudio,
        engine_rate: u32,
        beatgrid: Beatgrid,
        cues: &[Cue],
        auto_gain_db: f32,
    ) -> Arc<Self> {
        let stereo: Vec<f32> = match audio.channels {
            2 => audio.samples,
            1 => audio.samples.iter().flat_map(|&v| [v, v]).collect(),
            n if n > 2 => audio
                .samples
                .chunks_exact(n as usize)
                .flat_map(|f| [f[0], f[1]])
                .collect(),
            _ => Vec::new(),
        };
        let samples = if audio.sample_rate != engine_rate && audio.sample_rate > 0 {
            crate::dsp::resample::resample_stereo(&stereo, audio.sample_rate, engine_rate)
        } else {
            stereo
        };
        let mut hot_cues = [None; HOT_CUES];
        let mut memory: Vec<HotCue> = Vec::new();
        for c in cues {
            let hc = HotCue {
                start: c.start,
                loop_end: c.loop_end,
            };
            match c.kind {
                CueKind::Hot(slot) if (slot as usize) < HOT_CUES => {
                    hot_cues[slot as usize] = Some(hc)
                }
                CueKind::Hot(_) => {}
                CueKind::Memory => memory.push(hc),
            }
        }
        memory.sort_by(|a, b| a.start.total_cmp(&b.start));
        Arc::new(Self {
            id,
            sample_rate: engine_rate,
            samples: samples.into_boxed_slice(),
            beatgrid,
            hot_cues,
            memory_cues: memory.into_boxed_slice(),
            auto_gain_db,
        })
    }

    /// Decodes `path` and prepares it for an engine running at `engine_rate`.
    pub fn load(
        id: TrackId,
        path: &Path,
        engine_rate: u32,
        beatgrid: Beatgrid,
        cues: &[Cue],
        auto_gain_db: f32,
    ) -> Result<Arc<Self>, DecodeError> {
        let audio = od_decode::decode(path, Layout::Stereo)?;
        Ok(Self::new(
            id,
            audio,
            engine_rate,
            beatgrid,
            cues,
            auto_gain_db,
        ))
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }
}

/// A sampler sample, interleaved stereo at the engine rate.
#[derive(Debug)]
pub struct SampleData {
    pub samples: Box<[f32]>,
    pub sample_rate: u32,
}

impl SampleData {
    pub fn new(audio: DecodedAudio, engine_rate: u32) -> Arc<Self> {
        let t = TrackData::new(
            TrackId(0),
            audio,
            engine_rate,
            Beatgrid::default(),
            &[],
            0.0,
        );
        let samples = t.samples.clone();
        Arc::new(Self {
            samples,
            sample_rate: engine_rate,
        })
    }

    pub fn load(path: &Path, engine_rate: u32) -> Result<Arc<Self>, DecodeError> {
        Ok(Self::new(
            od_decode::decode(path, Layout::Stereo)?,
            engine_rate,
        ))
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}
