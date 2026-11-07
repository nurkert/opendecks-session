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

use crate::config::{HOT_CUE_BANKS, HOT_CUES};

/// Level (peak, linear) above which auto cue considers the track audible
/// (−54 dBFS).
pub const AUTO_CUE_THRESHOLD: f32 = 0.002;
use crate::stems::{StemData, StemInput};

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
    /// Hot cues as stored in the library: `hot_cues[bank][slot]` holds
    /// library hot cue `bank * 8 + slot` (A–H of bank 0, …).
    pub hot_cues: [[Option<HotCue>; HOT_CUES]; HOT_CUE_BANKS],
    /// Memory cue (and memory loop) start positions in seconds, sorted.
    pub memory_cues: Box<[HotCue]>,
    /// Auto-gain in dB derived from loudness analysis (0 if unknown).
    pub auto_gain_db: f32,
    /// First audible position in seconds (peak above
    /// [`AUTO_CUE_THRESHOLD`]), used by auto cue; 0 for silent tracks.
    pub first_sound: f64,
    /// Separated vocals/drums/bass at the engine rate, if available
    /// (enables [`crate::DeckCommand::SetStemLevel`]).
    pub stems: Option<StemData>,
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
        Self::new_with_stems(id, audio, engine_rate, beatgrid, cues, auto_gain_db, None)
    }

    /// Like [`TrackData::new`], additionally attaching separated stems
    /// (resampled to `engine_rate` like the mix). `stems` must be aligned
    /// with `audio` (same start, normally the same rate).
    pub fn new_with_stems(
        id: TrackId,
        audio: DecodedAudio,
        engine_rate: u32,
        beatgrid: Beatgrid,
        cues: &[Cue],
        auto_gain_db: f32,
        stems: Option<&StemInput>,
    ) -> Arc<Self> {
        let stems = stems.map(|s| StemData::new(s, engine_rate));
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
        let mut hot_cues = [[None; HOT_CUES]; HOT_CUE_BANKS];
        let mut memory: Vec<HotCue> = Vec::new();
        for c in cues {
            let hc = HotCue {
                start: c.start,
                loop_end: c.loop_end,
            };
            match c.kind {
                CueKind::Hot(slot) if (slot as usize) < HOT_CUES * HOT_CUE_BANKS => {
                    let slot = slot as usize;
                    hot_cues[slot / HOT_CUES][slot % HOT_CUES] = Some(hc)
                }
                CueKind::Hot(_) => {}
                CueKind::Memory => memory.push(hc),
            }
        }
        memory.sort_by(|a, b| a.start.total_cmp(&b.start));
        let first_sound = samples
            .iter()
            .position(|v| v.abs() > AUTO_CUE_THRESHOLD)
            .map_or(0.0, |i| (i / 2) as f64 / engine_rate as f64);
        Arc::new(Self {
            id,
            sample_rate: engine_rate,
            samples: samples.into_boxed_slice(),
            beatgrid,
            hot_cues,
            memory_cues: memory.into_boxed_slice(),
            auto_gain_db,
            first_sound,
            stems,
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

    /// Decodes `path` and attaches `stems` (see [`TrackData::new_with_stems`];
    /// `od_stems::StemCache` provides them for cached tracks).
    #[allow(clippy::too_many_arguments)]
    pub fn load_with_stems(
        id: TrackId,
        path: &Path,
        engine_rate: u32,
        beatgrid: Beatgrid,
        cues: &[Cue],
        auto_gain_db: f32,
        stems: Option<&StemInput>,
    ) -> Result<Arc<Self>, DecodeError> {
        let audio = od_decode::decode(path, Layout::Stereo)?;
        Ok(Self::new_with_stems(
            id,
            audio,
            engine_rate,
            beatgrid,
            cues,
            auto_gain_db,
            stems,
        ))
    }

    /// A copy of this track with `stems` attached (stems arriving after a
    /// load, swapped in with [`crate::DeckCommand::SwapTrackData`]).
    pub fn with_stems(&self, stems: StemData) -> Arc<Self> {
        Arc::new(Self {
            id: self.id,
            sample_rate: self.sample_rate,
            samples: self.samples.clone(),
            beatgrid: self.beatgrid.clone(),
            hot_cues: self.hot_cues,
            memory_cues: self.memory_cues.clone(),
            auto_gain_db: self.auto_gain_db,
            first_sound: self.first_sound,
            stems: Some(stems),
        })
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn has_stems(&self) -> bool {
        self.stems.is_some()
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
