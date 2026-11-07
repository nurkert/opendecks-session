//! Neural separation with HT-Demucs (ONNX Runtime).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use od_decode::{DecodedAudio, Layout};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;

use crate::model::MODEL;
use crate::{Error, Stems};

/// Model sample rate.
pub const MODEL_RATE: u32 = 44_100;
/// Samples per model segment (7.8 s at 44.1 kHz; fixed by the export).
pub const SEGMENT: usize = 343_980;
/// Overlap between consecutive segments (fraction of a segment).
pub const OVERLAP: f32 = 0.25;
/// Model output order.
const SOURCES: [&str; 4] = ["drums", "bass", "other", "vocals"];

/// Options for [`Separator::new`].
#[derive(Clone, Debug)]
pub struct SeparatorOptions {
    /// ONNX Runtime intra-op threads (0 = all cores).
    pub threads: usize,
    /// Segment overlap 0..0.5 (more = smoother segment borders, slower).
    pub overlap: f32,
}

impl Default for SeparatorOptions {
    fn default() -> Self {
        Self {
            threads: 0,
            overlap: OVERLAP,
        }
    }
}

/// A loaded separation model. Creating one takes ~1 s and ~1 GB of RAM
/// while running; keep it around for several tracks (the [`crate::StemWorker`]
/// does).
pub struct Separator {
    session: Session,
    overlap: f32,
}

impl Separator {
    /// Loads the model from `model_path`.
    pub fn new(model_path: &Path, opts: &SeparatorOptions) -> Result<Self, Error> {
        let threads = if opts.threads == 0 {
            std::thread::available_parallelism().map_or(4, |n| n.get())
        } else {
            opts.threads
        };
        // CPU only: the CoreML execution provider (ORT 1.28) fails to
        // compile HT-Demucs as NeuralNetwork and segfaults as MLProgram.
        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort::Error::from)?
            .with_intra_threads(threads)
            .map_err(ort::Error::from)?
            .commit_from_file(model_path)?;
        Ok(Self {
            session,
            overlap: opts.overlap.clamp(0.0, 0.5),
        })
    }

    /// Loads the model from the default location (see [`crate::model_path`]);
    /// [`Error::ModelMissing`] if it was not fetched yet.
    pub fn load_default() -> Result<Self, Error> {
        let p = crate::model_path();
        if !crate::model_available() {
            return Err(Error::ModelMissing(p));
        }
        Self::new(&p, &SeparatorOptions::default())
    }

    /// Decodes `path` and separates it (see [`Separator::separate`]).
    pub fn separate_file(
        &mut self,
        path: &Path,
        progress: &mut dyn FnMut(f32),
        cancel: &AtomicBool,
    ) -> Result<Stems, Error> {
        let audio = od_decode::decode(path, Layout::Stereo)?;
        self.separate(&audio, progress, cancel)
    }

    /// Separates decoded audio into vocals, drums and bass, returned as
    /// interleaved stereo at the audio's own rate, sample-aligned with it.
    /// `progress` receives 0..=1; setting `cancel` aborts with
    /// [`Error::Cancelled`].
    pub fn separate(
        &mut self,
        audio: &DecodedAudio,
        progress: &mut dyn FnMut(f32),
        cancel: &AtomicBool,
    ) -> Result<Stems, Error> {
        let rate = audio.sample_rate;
        if rate == 0 {
            return Err(Error::Audio("sample rate 0".into()));
        }
        let stereo = to_stereo(audio);
        let input = od_engine::dsp::resample::resample_stereo(&stereo, rate, MODEL_RATE);
        let total = input.len() / 2;
        let overlap = ((SEGMENT as f32 * self.overlap) as usize).max(1);
        let stride = SEGMENT - overlap;
        // Segment starts: every `stride`, the last one aligned to the end so
        // no segment is mostly zero padding (the model normalises each
        // segment; near-silent padded segments produced loud artefacts).
        let mut starts: Vec<usize> = (0..)
            .map(|k| k * stride)
            .take_while(|s| s + SEGMENT < total)
            .collect();
        starts.push(total.saturating_sub(SEGMENT));
        if starts.len() > 1 && starts[starts.len() - 1] <= starts[starts.len() - 2] {
            starts.pop();
        }
        let chunks = starts.len();
        // Fade-in/out weights for overlap-add; never 0 so edges stay defined.
        let window: Vec<f32> = (0..SEGMENT)
            .map(|i| {
                let a = (i + 1) as f32 / (overlap + 1) as f32;
                let b = (SEGMENT - i) as f32 / (overlap + 1) as f32;
                a.min(b).min(1.0)
            })
            .collect();
        // Planar accumulators: [vocals, drums, bass] × [L, R].
        let mut acc: Vec<Vec<f32>> = (0..6).map(|_| vec![0f32; total]).collect();
        let mut weight = vec![0f32; total];
        let mut seg = vec![0f32; 2 * SEGMENT];
        progress(0.0);
        for (ci, &start) in starts.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let end = (start + SEGMENT).min(total);
            let len = end - start;
            seg.fill(0.0);
            for i in 0..len {
                seg[i] = input[(start + i) * 2];
                seg[SEGMENT + i] = input[(start + i) * 2 + 1];
            }
            let out = self
                .session
                .run(ort::inputs!["mix" => TensorRef::from_array_view(([1usize, 2, SEGMENT], &seg[..]))?])?;
            let (shape, data) = out["stems"].try_extract_tensor::<f32>()?;
            if shape.num_elements() != 4 * 2 * SEGMENT {
                return Err(Error::Model(format!("unexpected output shape {shape:?}")));
            }
            // Output [1, 4, 2, N] in SOURCES order.
            for (k, src) in [3usize, 0, 1].into_iter().enumerate() {
                debug_assert!(matches!(SOURCES[src], "vocals" | "drums" | "bass"));
                for c in 0..2 {
                    let o = &data[(src * 2 + c) * SEGMENT..(src * 2 + c) * SEGMENT + len];
                    let a = &mut acc[k * 2 + c][start..end];
                    for i in 0..len {
                        a[i] += o[i] * window[i];
                    }
                }
            }
            for i in 0..len {
                weight[start + i] += window[i];
            }
            progress((ci + 1) as f32 / chunks as f32 * 0.95);
        }
        let mut parts: [Vec<f32>; 3] = std::array::from_fn(|k| {
            let (l, r) = (&acc[k * 2], &acc[k * 2 + 1]);
            let mut v = vec![0f32; total * 2];
            for i in 0..total {
                let w = weight[i].max(1e-8);
                v[i * 2] = l[i] / w;
                v[i * 2 + 1] = r[i] / w;
            }
            v
        });
        drop(acc);
        // Back to the track rate, trimmed/padded to the mix length.
        let frames = stereo.len() / 2;
        for p in &mut parts {
            let mut v = od_engine::dsp::resample::resample_stereo(p, MODEL_RATE, rate);
            v.resize(frames * 2, 0.0);
            *p = v;
        }
        progress(1.0);
        let [vocals, drums, bass] = parts;
        Ok(Stems {
            sample_rate: rate,
            vocals,
            drums,
            bass,
        })
    }
}

fn to_stereo(audio: &DecodedAudio) -> std::borrow::Cow<'_, [f32]> {
    match audio.channels {
        2 => (&audio.samples[..]).into(),
        1 => audio
            .samples
            .iter()
            .flat_map(|&v| [v, v])
            .collect::<Vec<_>>()
            .into(),
        n if n > 2 => audio
            .samples
            .chunks_exact(n as usize)
            .flat_map(|f| [f[0], f[1]])
            .collect::<Vec<_>>()
            .into(),
        _ => Vec::new().into(),
    }
}

/// One-shot convenience: loads the default model and separates `path`.
/// Prefer a long-lived [`Separator`] (or the [`crate::StemWorker`]) for
/// several tracks.
pub fn separate(
    path: &Path,
    progress: &mut dyn FnMut(f32),
    cancel: &AtomicBool,
) -> Result<Stems, Error> {
    let _ = MODEL;
    Separator::load_default()?.separate_file(path, progress, cancel)
}
