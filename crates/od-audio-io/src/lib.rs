//! Audio device I/O for OpenDecks Session (SPEC §4.2, §10.3).
//!
//! * macOS: CoreAudio (through `cpal`).
//! * Linux / Raspberry Pi: ALSA directly through cpal's ALSA host (use a
//!   `hw:`/`plughw:` device name for the low-latency Lite path, enable the
//!   `realtime` feature for RT thread priority); PipeWire (`pipewire`
//!   feature) or JACK (`jack` feature) on desktops.
//!
//! [`AudioHost::start`] opens one (multi-channel) output device, runs
//! [`od_engine::EngineProcessor::process`] in the device callback and routes
//! master and headphone stereo pairs to configurable device channels. The
//! callback never allocates or blocks; it converts to the device's sample
//! format and counts xruns.

mod devices;
mod input;
mod output;

pub use devices::{Backend, DeviceInfo, backends, default_output_device_name, list_devices};
pub use input::{AudioInput, InputConfig};
pub use output::{AudioConfig, AudioHost, AudioStats, Negotiated, OutputRouting, StreamInfo};

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("audio backend {0:?} is not available on this system")]
    BackendUnavailable(Backend),
    #[error("no audio device found matching {0:?}")]
    DeviceNotFound(String),
    #[error("device {device} does not support {sample_rate} Hz with {channels} channels")]
    UnsupportedConfig {
        device: String,
        sample_rate: u32,
        channels: u16,
    },
    #[error("unsupported device sample format {0}")]
    UnsupportedFormat(String),
    #[error("audio backend error: {0}")]
    Backend(#[from] cpal::Error),
}

pub type Result<T> = std::result::Result<T, AudioError>;
