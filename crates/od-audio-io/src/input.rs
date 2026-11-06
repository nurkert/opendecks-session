//! Input (capture) streams, delivered lock-free as interleaved `f32`.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::devices::{Backend, device_name, find_device, host};
use crate::{AudioError, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputConfig {
    pub backend: Backend,
    /// Device name or id substring; `None` = default input.
    pub device: Option<String>,
    pub sample_rate: Option<u32>,
    pub buffer_frames: Option<u32>,
    /// Number of channels to capture; `None` = device maximum.
    pub channels: Option<u16>,
    /// Ring buffer length in seconds (default 2).
    pub ring_seconds: Option<u32>,
}

/// A running capture stream. Samples arrive in the consumer returned by
/// [`AudioInput::start`].
pub struct AudioInput {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    dropped: Arc<AtomicU64>,
    xruns: Arc<AtomicU64>,
}

impl AudioInput {
    pub fn start(cfg: &InputConfig) -> Result<(AudioInput, rtrb::Consumer<f32>)> {
        let h = host(cfg.backend)?;
        let dev = match cfg.device.as_ref().filter(|n| !n.trim().is_empty()) {
            Some(n) => {
                find_device(&h, n, false).ok_or_else(|| AudioError::DeviceNotFound(n.clone()))?
            }
            None => h
                .default_input_device()
                .ok_or_else(|| AudioError::DeviceNotFound("default input".into()))?,
        };
        let name = device_name(&dev);
        let default = dev.default_input_config()?;
        let rate = cfg.sample_rate.unwrap_or(default.sample_rate());
        let range = dev
            .supported_input_configs()?
            .filter(|r| r.contains_rate(rate) && cfg.channels.is_none_or(|c| r.channels() >= c))
            .max_by_key(|r| (r.sample_format() == SampleFormat::F32, r.channels()))
            .ok_or(AudioError::UnsupportedConfig {
                device: name.clone(),
                sample_rate: rate,
                channels: cfg.channels.unwrap_or(2),
            })?;
        let channels = cfg
            .channels
            .unwrap_or(range.channels())
            .min(range.channels());
        let stream_cfg = cpal::StreamConfig {
            channels: range.channels(),
            sample_rate: rate,
            buffer_size: cfg
                .buffer_frames
                .map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
        };
        let cap = rate as usize * channels as usize * cfg.ring_seconds.unwrap_or(2) as usize;
        let (tx, rx) = rtrb::RingBuffer::new(cap.max(1024));
        let dropped = Arc::new(AtomicU64::new(0));
        let xruns = Arc::new(AtomicU64::new(0));
        let stream = match range.sample_format() {
            SampleFormat::F32 => typed::<f32>(
                &dev,
                &stream_cfg,
                channels,
                tx,
                dropped.clone(),
                xruns.clone(),
            ),
            SampleFormat::I32 => typed::<i32>(
                &dev,
                &stream_cfg,
                channels,
                tx,
                dropped.clone(),
                xruns.clone(),
            ),
            SampleFormat::I24 => typed::<cpal::I24>(
                &dev,
                &stream_cfg,
                channels,
                tx,
                dropped.clone(),
                xruns.clone(),
            ),
            SampleFormat::I16 => typed::<i16>(
                &dev,
                &stream_cfg,
                channels,
                tx,
                dropped.clone(),
                xruns.clone(),
            ),
            SampleFormat::U16 => typed::<u16>(
                &dev,
                &stream_cfg,
                channels,
                tx,
                dropped.clone(),
                xruns.clone(),
            ),
            other => return Err(AudioError::UnsupportedFormat(format!("{other:?}"))),
        }?;
        stream.play()?;
        Ok((
            AudioInput {
                _stream: stream,
                device_name: name,
                sample_rate: rate,
                channels,
                dropped,
                xruns,
            },
            rx,
        ))
    }

    /// Samples dropped because the consumer did not keep up.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn xruns(&self) -> u64 {
        self.xruns.load(Ordering::Relaxed)
    }
}

fn typed<T: SizedSample>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    keep: u16,
    mut tx: rtrb::Producer<f32>,
    dropped: Arc<AtomicU64>,
    xruns: Arc<AtomicU64>,
) -> Result<cpal::Stream>
where
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    let keep = keep as usize;
    Ok(dev.build_input_stream::<T, _, _>(
        *cfg,
        move |data: &[T], _| {
            for frame in data.chunks_exact(ch) {
                for s in &frame[..keep] {
                    if tx.push(s.to_sample::<f32>()).is_err() {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        },
        move |e: cpal::Error| {
            if e.kind() == cpal::ErrorKind::Xrun {
                xruns.fetch_add(1, Ordering::Relaxed);
            } else {
                tracing::warn!(error = %e, "audio input error");
            }
        },
        None,
    )?)
}
