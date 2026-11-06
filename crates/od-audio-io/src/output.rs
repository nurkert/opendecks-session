//! Output stream running the engine.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputCallbackInfo, SampleFormat, SizedSample};
use od_engine::{EngineProcessor, Outputs};

use crate::devices::{Backend, device_name, find_device, host};
use crate::{AudioError, Result};

/// Engine render scratch size; device callbacks larger than this are split.
const SCRATCH_FRAMES: usize = 8192;

/// Which device channels (0-based) receive master and headphone stereo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputRouting {
    pub master: [u16; 2],
    /// `None` disables the headphone output.
    pub headphones: Option<[u16; 2]>,
}

impl OutputRouting {
    /// Master on 1/2; headphones on 3/4 if the device has at least four
    /// output channels (DDJ-FLX10 / FLX4 layout).
    pub fn auto(device_channels: u16) -> Self {
        Self {
            master: [0, 1],
            headphones: (device_channels >= 4).then_some([2, 3]),
        }
    }

    fn channels_needed(&self) -> u16 {
        let m = self.master[0].max(self.master[1]);
        let h = self.headphones.map_or(0, |h| h[0].max(h[1]));
        m.max(h) + 1
    }
}

/// What to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioConfig {
    pub backend: Backend,
    /// Device name or id substring (case-insensitive); `None` = default output.
    pub device: Option<String>,
    /// Requested sample rate; `None` = device default (48 kHz preferred).
    /// [`AudioHost::start`] always uses the engine's rate.
    pub sample_rate: Option<u32>,
    /// Requested buffer size in frames; `None` = backend default.
    pub buffer_frames: Option<u32>,
    /// `None` = [`OutputRouting::auto`].
    pub routing: Option<OutputRouting>,
    /// If the requested device is missing or cannot be opened, use the
    /// default output for master only (headphones disabled).
    pub fallback_to_default: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            backend: Backend::Default,
            device: None,
            sample_rate: None,
            buffer_frames: None,
            routing: None,
            fallback_to_default: true,
        }
    }
}

/// The configuration a device will actually run with.
#[derive(Clone, Debug, PartialEq)]
pub struct Negotiated {
    pub backend: Backend,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_frames: Option<u32>,
    pub sample_format: String,
    pub routing: OutputRouting,
    /// True if the requested device was unavailable and the default was used.
    pub fell_back: bool,
}

/// Information about a running stream.
pub type StreamInfo = Negotiated;

impl Negotiated {
    /// Nominal output latency from the buffer size (excluding converter /
    /// device latency, see [`AudioStats::output_latency`]).
    pub fn nominal_latency(&self) -> Option<Duration> {
        self.buffer_frames
            .map(|b| Duration::from_secs_f64(b as f64 / self.sample_rate as f64))
    }
}

#[derive(Default)]
struct SharedStats {
    callbacks: AtomicU64,
    xruns: AtomicU64,
    overruns: AtomicU64,
    last_frames: AtomicU32,
    max_frames: AtomicU32,
    latency_ns: AtomicU64,
    load_ppm: AtomicU32,
    peak_load_ppm: AtomicU32,
    device_lost: AtomicBool,
}

/// Runtime statistics of the output stream.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioStats {
    pub callbacks: u64,
    /// Underruns/overruns reported by the backend.
    pub xruns: u64,
    /// Callbacks whose processing took longer than the buffer duration.
    pub overruns: u64,
    /// Frames of the last callback.
    pub callback_frames: u32,
    pub max_callback_frames: u32,
    /// Callback-to-playback delay reported by the backend (actual output
    /// latency), if available.
    pub output_latency: Option<Duration>,
    /// Engine processing time / buffer duration of the last callback (0..1+).
    pub load: f32,
    /// Highest load seen since start.
    pub peak_load: f32,
    pub device_lost: bool,
}

struct CallbackState {
    processor: EngineProcessor,
    master: Vec<f32>,
    phones: Vec<f32>,
    channels: usize,
    routing: OutputRouting,
    sample_rate: f64,
    stats: Arc<SharedStats>,
}

impl CallbackState {
    fn render<T: SizedSample + FromSample<f32>>(
        &mut self,
        data: &mut [T],
        info: &OutputCallbackInfo,
    ) {
        let t0 = Instant::now();
        let ch = self.channels;
        let frames = data.len() / ch;
        let silence = T::from_sample(0.0f32);
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(SCRATCH_FRAMES);
            let hp_enabled = self.routing.headphones.is_some();
            {
                let mut out = Outputs {
                    master: &mut self.master[..n * 2],
                    headphones: if hp_enabled {
                        Some(&mut self.phones[..n * 2])
                    } else {
                        None
                    },
                };
                self.processor.process(n, &mut out);
            }
            let [ml, mr] = self.routing.master.map(|c| c as usize);
            let hp = self.routing.headphones.map(|h| h.map(|c| c as usize));
            for (i, frame) in data[done * ch..(done + n) * ch]
                .chunks_exact_mut(ch)
                .enumerate()
            {
                frame.iter_mut().for_each(|s| *s = silence);
                frame[ml] = T::from_sample(self.master[i * 2]);
                frame[mr] = T::from_sample(self.master[i * 2 + 1]);
                if let Some([hl, hr]) = hp {
                    frame[hl] = T::from_sample(self.phones[i * 2]);
                    frame[hr] = T::from_sample(self.phones[i * 2 + 1]);
                }
            }
            done += n;
        }
        // Statistics (atomics only).
        let s = &self.stats;
        s.callbacks.fetch_add(1, Ordering::Relaxed);
        s.last_frames.store(frames as u32, Ordering::Relaxed);
        s.max_frames.fetch_max(frames as u32, Ordering::Relaxed);
        let ts = info.timestamp();
        let lat = ts.playback.saturating_duration_since(ts.callback);
        s.latency_ns.store(lat.as_nanos() as u64, Ordering::Relaxed);
        let budget = frames as f64 / self.sample_rate;
        if budget > 0.0 {
            let load = t0.elapsed().as_secs_f64() / budget;
            let ppm = (load * 1e6) as u32;
            s.load_ppm.store(ppm, Ordering::Relaxed);
            s.peak_load_ppm.fetch_max(ppm, Ordering::Relaxed);
            if load > 1.0 {
                s.overruns.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// A running output stream driving the engine. Dropping it stops audio.
///
/// Note: on some backends the underlying stream must be dropped on the
/// thread that created it; keep the `AudioHost` on that thread.
pub struct AudioHost {
    _stream: cpal::Stream,
    info: StreamInfo,
    stats: Arc<SharedStats>,
}

fn pick_device(cfg: &AudioConfig) -> Result<(cpal::Device, bool)> {
    let h = host(cfg.backend)?;
    if let Some(name) = cfg.device.as_ref().filter(|n| !n.trim().is_empty()) {
        if let Some(d) = find_device(&h, name, true) {
            return Ok((d, false));
        }
        if !cfg.fallback_to_default {
            return Err(AudioError::DeviceNotFound(name.clone()));
        }
        tracing::warn!(device = %name, "audio device not found, falling back to default output");
        let d = h
            .default_output_device()
            .ok_or_else(|| AudioError::DeviceNotFound("default".into()))?;
        return Ok((d, true));
    }
    let d = h
        .default_output_device()
        .ok_or_else(|| AudioError::DeviceNotFound("default".into()))?;
    Ok((d, false))
}

fn format_rank(f: SampleFormat) -> u8 {
    match f {
        SampleFormat::F32 => 0,
        SampleFormat::I32 => 1,
        SampleFormat::I24 => 2,
        SampleFormat::I16 => 3,
        SampleFormat::F64 => 4,
        SampleFormat::U16 => 5,
        _ => 9,
    }
}

fn negotiate_on(
    cfg: &AudioConfig,
    device: &cpal::Device,
    fell_back: bool,
    force_rate: Option<u32>,
) -> Result<(Negotiated, cpal::StreamConfig, SampleFormat)> {
    let name = device_name(device);
    let ranges: Vec<_> = device.supported_output_configs()?.collect();
    let max_ch = ranges.iter().map(|r| r.channels()).max().unwrap_or(0);
    let mut routing = if fell_back {
        OutputRouting {
            master: [0, 1],
            headphones: None,
        }
    } else {
        cfg.routing.unwrap_or_else(|| OutputRouting::auto(max_ch))
    };
    if routing.channels_needed() > max_ch {
        // Not enough channels for the headphone pair: master only.
        tracing::warn!(device = %name, "device has {max_ch} outputs; headphone output disabled");
        routing.headphones = None;
    }
    let needed = routing.channels_needed().max(2);
    let default_rate = device.default_output_config().ok().map(|c| c.sample_rate());
    let rate = force_rate
        .or(cfg.sample_rate)
        .or(default_rate.filter(|r| *r == 44_100 || *r == 48_000))
        .unwrap_or(48_000);
    let best = ranges
        .iter()
        .filter(|r| {
            r.channels() >= needed && r.contains_rate(rate) && format_rank(r.sample_format()) < 9
        })
        .min_by_key(|r| (r.channels(), format_rank(r.sample_format())))
        .ok_or_else(|| AudioError::UnsupportedConfig {
            device: name.clone(),
            sample_rate: rate,
            channels: needed,
        })?;
    let buffer = match (cfg.buffer_frames, best.buffer_size()) {
        (Some(b), cpal::SupportedBufferSize::Range { min, max }) => Some(b.clamp(*min, *max)),
        (Some(b), _) => Some(b),
        (None, _) => None,
    };
    let stream_cfg = cpal::StreamConfig {
        channels: best.channels(),
        sample_rate: rate,
        buffer_size: buffer.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
    };
    let resolved_backend = cfg.backend;
    Ok((
        Negotiated {
            backend: resolved_backend,
            device_name: name,
            sample_rate: rate,
            channels: best.channels(),
            buffer_frames: buffer,
            sample_format: format!("{:?}", best.sample_format()),
            routing,
            fell_back,
        },
        stream_cfg,
        best.sample_format(),
    ))
}

impl AudioHost {
    /// Resolves device, channel count, sample rate and buffer size without
    /// opening a stream. Create the engine with the returned sample rate.
    pub fn negotiate(cfg: &AudioConfig) -> Result<Negotiated> {
        let (dev, fell_back) = pick_device(cfg)?;
        negotiate_on(cfg, &dev, fell_back, None).map(|(n, _, _)| n)
    }

    /// Opens the output device and starts running `processor` in its
    /// callback. The device runs at the engine's sample rate.
    pub fn start(cfg: &AudioConfig, processor: EngineProcessor) -> Result<AudioHost> {
        let rate = processor.config().sample_rate;
        let (dev, fell_back) = pick_device(cfg)?;
        let attempt = negotiate_on(cfg, &dev, fell_back, Some(rate));
        let (info, stream_cfg, fmt, dev) = match attempt {
            Ok((n, s, f)) => (n, s, f, dev),
            Err(e) if cfg.fallback_to_default && !fell_back => {
                tracing::warn!(error = %e, "cannot open requested device, using default output");
                let h = host(cfg.backend)?;
                let d = h.default_output_device().ok_or(e)?;
                let (n, s, f) = negotiate_on(cfg, &d, true, Some(rate))?;
                (n, s, f, d)
            }
            Err(e) => return Err(e),
        };
        let stats = Arc::new(SharedStats::default());
        let state = CallbackState {
            processor,
            master: vec![0.0; SCRATCH_FRAMES * 2],
            phones: vec![0.0; SCRATCH_FRAMES * 2],
            channels: stream_cfg.channels as usize,
            routing: info.routing,
            sample_rate: rate as f64,
            stats: stats.clone(),
        };
        let stream = build(&dev, &stream_cfg, fmt, state, stats.clone())?;
        stream.play()?;
        tracing::info!(
            device = %info.device_name, rate = info.sample_rate, channels = info.channels,
            buffer = ?info.buffer_frames, format = %info.sample_format, "audio output started"
        );
        Ok(AudioHost {
            _stream: stream,
            info,
            stats,
        })
    }

    pub fn info(&self) -> &StreamInfo {
        &self.info
    }

    pub fn stats(&self) -> AudioStats {
        let s = &self.stats;
        let lat = s.latency_ns.load(Ordering::Relaxed);
        AudioStats {
            callbacks: s.callbacks.load(Ordering::Relaxed),
            xruns: s.xruns.load(Ordering::Relaxed),
            overruns: s.overruns.load(Ordering::Relaxed),
            callback_frames: s.last_frames.load(Ordering::Relaxed),
            max_callback_frames: s.max_frames.load(Ordering::Relaxed),
            output_latency: (lat > 0).then(|| Duration::from_nanos(lat)),
            load: s.load_ppm.load(Ordering::Relaxed) as f32 / 1e6,
            peak_load: s.peak_load_ppm.load(Ordering::Relaxed) as f32 / 1e6,
            device_lost: s.device_lost.load(Ordering::Relaxed),
        }
    }
}

fn build(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    fmt: SampleFormat,
    state: CallbackState,
    stats: Arc<SharedStats>,
) -> Result<cpal::Stream> {
    fn typed<T: SizedSample + FromSample<f32>>(
        dev: &cpal::Device,
        cfg: &cpal::StreamConfig,
        mut state: CallbackState,
        stats: Arc<SharedStats>,
    ) -> Result<cpal::Stream> {
        let err = move |e: cpal::Error| match e.kind() {
            cpal::ErrorKind::Xrun => {
                stats.xruns.fetch_add(1, Ordering::Relaxed);
            }
            cpal::ErrorKind::DeviceNotAvailable => {
                stats.device_lost.store(true, Ordering::Relaxed);
                tracing::error!("audio device disconnected");
            }
            _ => tracing::warn!(error = %e, "audio stream error"),
        };
        Ok(dev.build_output_stream::<T, _, _>(
            *cfg,
            move |data: &mut [T], info: &OutputCallbackInfo| state.render(data, info),
            err,
            None,
        )?)
    }
    match fmt {
        SampleFormat::F32 => typed::<f32>(dev, cfg, state, stats),
        SampleFormat::I32 => typed::<i32>(dev, cfg, state, stats),
        SampleFormat::I24 => typed::<cpal::I24>(dev, cfg, state, stats),
        SampleFormat::I16 => typed::<i16>(dev, cfg, state, stats),
        SampleFormat::F64 => typed::<f64>(dev, cfg, state, stats),
        SampleFormat::U16 => typed::<u16>(dev, cfg, state, stats),
        other => Err(AudioError::UnsupportedFormat(format!("{other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_routing() {
        assert_eq!(
            OutputRouting::auto(2),
            OutputRouting {
                master: [0, 1],
                headphones: None
            }
        );
        let r = OutputRouting::auto(4);
        assert_eq!(r.headphones, Some([2, 3]));
        assert_eq!(r.channels_needed(), 4);
    }
}
