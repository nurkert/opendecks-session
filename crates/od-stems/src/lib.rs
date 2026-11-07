//! Stem separation for OpenDecks Session: splits tracks into vocals, drums
//! and bass (the instrumental remainder is implicit: mix − the three), caches
//! the result per track, and hands it to the engine for live part
//! muting (part isolator).
//!
//! # Model and licences
//!
//! Separation uses **HT-Demucs v4** (`htdemucs`, Rouard, Massa & Défossez,
//! "Hybrid Transformers for Music Source Separation", ICASSP 2023), a single
//! 4-source model (drums, bass, other, vocals; median SDR on MUSDB18-HQ ≈ 8.8
//! dB vocals / 9.5 dB drums). Code and pretrained weights:
//! <https://github.com/facebookresearch/demucs>, **MIT License**, © Meta
//! Platforms, Inc. The ONNX export with fp16-stored weights (STFT done with
//! convolution kernels inside the graph) comes from
//! <https://huggingface.co/StemSplitio/htdemucs-onnx> (**MIT License**), file
//! `htdemucs_fp16weights.onnx`, 165,612,636 bytes, SHA-256 `d05c269d…db70a`
//! (see [`MODEL`]). MIT is compatible with distribution alongside this
//! GPL-3.0-or-later software. The model is **not** in git: it is downloaded
//! on first use (or with `od-cli stems fetch-model`) to
//! `od_library::data_dir()/models/` and its SHA-256 verified.
//!
//! Inference runs in ONNX Runtime (MIT) through the `ort` crate (MIT OR
//! Apache-2.0) on the CPU (the CoreML execution provider of ONNX Runtime
//! 1.28 cannot run this model: compile error / segfault).
//!
//! # Features
//!
//! * `separation` (default): the model, [`Separator`], [`StemWorker`],
//!   [`fetch_model`]. Builds without it (e.g. the Raspberry Pi Lite profile)
//!   can still **play** precomputed stems: [`StemCache`] and
//!   [`load_track_data`] are always available.
//!
//! # API for the UI
//!
//! ```no_run
//! # #[cfg(feature = "separation")] {
//! use od_stems::*;
//! # let (id, path, hash, grid, cues) = (od_core::TrackId(1), std::path::PathBuf::new(), od_core::ContentHash(String::new()), od_core::Beatgrid::default(), Vec::new());
//! let cache = StemCache::open_default();          // data_dir()/stems
//!
//! // 1. Loading a deck: attaches cached stems if present (else plain track).
//! let track = load_track_data(&cache, id, &path, Some(&hash), 48_000, grid, &cues, 0.0)?;
//! let has_stems = track.has_stems();              // also DeckState::stems_available
//! // engine.deck(0, od_engine::DeckCommand::Load(track));
//!
//! // 2. Live control (any thread that owns an engine CommandSender):
//! // engine.deck(0, od_engine::DeckCommand::SetStemLevel { stem: Stem::Vocals, level: 0.0 });
//! // DeckState::stem_levels[Stem::Vocals.index()] reports the target level.
//!
//! // 3. Separating in the background (one long-lived worker per app):
//! let worker = StemWorker::spawn(cache.clone(), StemWorkerOptions::default());
//! worker.enqueue(StemJob { track: id, path: path.clone(), hash: hash.clone() });
//! for ev in worker.events().try_iter() {
//!     match ev {
//!         StemEvent::Progress { track, fraction } => { /* progress bar */ }
//!         StemEvent::Done { track, .. } | StemEvent::AlreadyCached { track, .. } => {
//!             // If `track` is on a deck, reload it with load_track_data()
//!             // (keeping position) to enable the stem buttons.
//!         }
//!         StemEvent::Failed { track, error } => { /* show error */ }
//!         _ => {}
//!     }
//! }
//! # }
//! # Ok::<(), od_stems::Error>(())
//! ```
//!
//! * [`StemWorker::enqueue`] / [`StemWorker::prioritize`] /
//!   [`StemWorker::cancel`] / [`StemWorker::cancel_all`]; jobs run one at a
//!   time (ONNX Runtime already uses all cores). The worker downloads the
//!   model on first use (reported as [`StemEvent::ModelDownload`]) unless
//!   [`StemWorkerOptions::fetch_model`] is false.
//! * [`StemCache::exists`] / [`StemCache::info`] are cheap (UI badges);
//!   [`StemCache::load`] decompresses the whole file.
//! * One-shot use: [`separate()`] or a [`Separator`] (`separate_file` /
//!   `separate` on [`od_decode::DecodedAudio`]) with a progress callback and
//!   a cancel flag.
//!
//! Engine side (crate `od-engine`): [`od_engine::TrackData::stems`],
//! [`od_engine::DeckCommand::SetStemLevel`], [`od_engine::Stem`],
//! `DeckState::{stems_available, stem_levels}`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use od_core::{Beatgrid, ContentHash, Cue, TrackId};
use od_engine::TrackData;

mod cache;
mod model;
#[cfg(onnx)]
mod separate;
#[cfg(all(feature = "separation", not(onnx)))]
#[path = "separate_unsupported.rs"]
mod separate;
#[cfg(feature = "separation")]
mod worker;

pub use cache::{FORMAT_VERSION, StemCache, StemFileInfo};
#[cfg(feature = "separation")]
pub use model::fetch_model;
pub use model::{MODEL, ModelInfo, model_available, model_path, models_dir};
pub use od_engine::Stem;
#[cfg(feature = "separation")]
pub use separate::{MODEL_RATE, OVERLAP, SEGMENT, Separator, SeparatorOptions, separate};
#[cfg(feature = "separation")]
pub use worker::{StemEvent, StemJob, StemWorker, StemWorkerOptions};

/// Separated stems: interleaved stereo `f32` at `sample_rate` (the track's
/// own rate), sample-aligned with the decoded mix. Inst/other is implicit
/// (mix − vocals − drums − bass).
pub type Stems = od_engine::StemInput;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("decoding: {0}")]
    Decode(#[from] od_decode::DecodeError),
    #[error("audio: {0}")]
    Audio(String),
    #[error("stem cache file: {0}")]
    CacheFormat(String),
    #[error("separation model not found at {0} (run `od-cli stems fetch-model`)")]
    ModelMissing(PathBuf),
    #[error("model download: {0}")]
    Download(String),
    #[error("model: {0}")]
    Model(String),
    #[error("cancelled")]
    Cancelled,
    #[error("stem separation is not available on this platform; stems separated on another computer still play")]
    Unsupported,
}

#[cfg(onnx)]
impl From<ort::Error> for Error {
    fn from(e: ort::Error) -> Self {
        Error::Model(e.to_string())
    }
}

/// Decodes a track for the engine and attaches its cached stems, if any
/// (`hash` = the library's content hash; `None` skips the lookup). A
/// corrupt cache file is logged and ignored (the track loads without stems).
#[allow(clippy::too_many_arguments)]
/// Decodes `path` for a deck without stems (the fast first step of a load;
/// see [`attach_cached_stems`]). Also returns the decoded rate and length,
/// which the cached stems must match.
#[allow(clippy::too_many_arguments)]
pub fn load_mix(
    id: TrackId,
    path: &Path,
    engine_rate: u32,
    beatgrid: Beatgrid,
    cues: &[Cue],
    auto_gain_db: f32,
) -> Result<(Arc<TrackData>, MixShape), Error> {
    let audio = od_decode::decode(path, od_decode::Layout::Stereo)?;
    let shape = MixShape { sample_rate: audio.sample_rate, frames: audio.frames() };
    Ok((TrackData::new_with_stems(id, audio, engine_rate, beatgrid, cues, auto_gain_db, None), shape))
}

/// Rate and length of a decoded mix ([`load_mix`]).
#[derive(Clone, Copy, Debug)]
pub struct MixShape {
    pub sample_rate: u32,
    pub frames: usize,
}

/// The second step of a load: `track` with the cached stems of `hash`
/// attached, if the cache has stems matching the mix.
pub fn attach_cached_stems(cache: &StemCache, track: &TrackData, hash: &ContentHash, shape: MixShape) -> Option<Arc<TrackData>> {
    let stems = match cache.load(hash) {
        Ok(s) => s?,
        Err(e) => {
            tracing::warn!("ignoring stem cache: {e}");
            return None;
        }
    };
    let ok = stems.sample_rate == shape.sample_rate && (stems.vocals.len() as i64 - shape.frames as i64 * 2).abs() <= 2 * 64;
    if !ok {
        tracing::warn!("stem cache does not match the audio; ignored");
        return None;
    }
    Some(track.with_stems(od_engine::StemData::new(&stems, track.sample_rate)))
}

pub fn load_track_data(
    cache: &StemCache,
    id: TrackId,
    path: &Path,
    hash: Option<&ContentHash>,
    engine_rate: u32,
    beatgrid: Beatgrid,
    cues: &[Cue],
    auto_gain_db: f32,
) -> Result<Arc<TrackData>, Error> {
    let stems = match hash.map(|h| cache.load(h)) {
        Some(Ok(s)) => s,
        Some(Err(e)) => {
            tracing::warn!("ignoring stem cache of {}: {e}", path.display());
            None
        }
        None => None,
    };
    let audio = od_decode::decode(path, od_decode::Layout::Stereo)?;
    let stems = stems.filter(|s| {
        let ok = s.sample_rate == audio.sample_rate
            && (s.vocals.len() as i64 - audio.frames() as i64 * 2).abs() <= 2 * 64;
        if !ok {
            tracing::warn!("stem cache of {} does not match the audio; ignored", path.display());
        }
        ok
    });
    Ok(TrackData::new_with_stems(
        id,
        audio,
        engine_rate,
        beatgrid,
        cues,
        auto_gain_db,
        stems.as_ref(),
    ))
}
