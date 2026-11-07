//! Backend and device enumeration.

use cpal::traits::{DeviceTrait, HostTrait};

use crate::{AudioError, Result};

/// Audio backend (cpal host).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Backend {
    /// Platform default: CoreAudio on macOS; PipeWire if compiled in and
    /// running, else ALSA on Linux.
    #[default]
    Default,
    CoreAudio,
    Alsa,
    Jack,
    PipeWire,
}

impl Backend {
    fn host_name(self) -> Option<&'static str> {
        match self {
            Backend::Default => None,
            Backend::CoreAudio => Some("CoreAudio"),
            Backend::Alsa => Some("ALSA"),
            Backend::Jack => Some("JACK"),
            Backend::PipeWire => Some("PipeWire"),
        }
    }

    fn from_host_name(name: &str) -> Option<Self> {
        [
            Backend::CoreAudio,
            Backend::Alsa,
            Backend::Jack,
            Backend::PipeWire,
        ]
        .into_iter()
        .find(|b| b.host_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
    }
}

/// Backends compiled in and available on this system.
pub fn backends() -> Vec<Backend> {
    cpal::available_hosts()
        .into_iter()
        .filter_map(|id| Backend::from_host_name(id.name()))
        .collect()
}

pub(crate) fn host(backend: Backend) -> Result<cpal::Host> {
    match backend.host_name() {
        None => Ok(cpal::default_host()),
        Some(name) => {
            let id = cpal::available_hosts()
                .into_iter()
                .find(|id| id.name().eq_ignore_ascii_case(name))
                .ok_or(AudioError::BackendUnavailable(backend))?;
            Ok(cpal::host_from_id(id)?)
        }
    }
}

pub(crate) fn device_name(d: &cpal::Device) -> String {
    d.description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| d.to_string())
}

/// Facts about one audio device.
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceInfo {
    pub backend: Backend,
    pub name: String,
    /// Stable backend-specific identifier.
    pub id: String,
    pub manufacturer: Option<String>,
    pub max_output_channels: u16,
    pub max_input_channels: u16,
    /// Supported standard rates among 44.1/48/88.2/96 kHz (output side).
    pub sample_rates: Vec<u32>,
    pub default_sample_rate: Option<u32>,
    /// Supported buffer size range in frames, if the backend reports it.
    pub buffer_range: Option<(u32, u32)>,
    pub is_default_output: bool,
    pub is_default_input: bool,
}

const RATES: [u32; 4] = [44_100, 48_000, 88_200, 96_000];

pub(crate) fn describe(
    backend: Backend,
    d: &cpal::Device,
    def_out: &str,
    def_in: &str,
) -> DeviceInfo {
    let name = device_name(d);
    let id = d.id().map(|i| i.to_string()).unwrap_or_default();
    let manufacturer = d
        .description()
        .ok()
        .and_then(|x| x.manufacturer().map(str::to_string));
    let mut max_out = 0;
    let mut rates = Vec::new();
    let mut buffer_range = None;
    if let Ok(cfgs) = d.supported_output_configs() {
        for c in cfgs {
            max_out = max_out.max(c.channels());
            for r in RATES {
                if c.contains_rate(r) && !rates.contains(&r) {
                    rates.push(r);
                }
            }
            if let cpal::SupportedBufferSize::Range { min, max } = *c.buffer_size() {
                buffer_range = Some((min, max));
            }
        }
    }
    rates.sort_unstable();
    let max_in = d
        .supported_input_configs()
        .map(|cfgs| cfgs.map(|c| c.channels()).max().unwrap_or(0))
        .unwrap_or(0);
    let default_sample_rate = d.default_output_config().ok().map(|c| c.sample_rate());
    DeviceInfo {
        backend,
        is_default_output: !def_out.is_empty() && name == def_out,
        is_default_input: !def_in.is_empty() && name == def_in,
        name,
        id,
        manufacturer,
        max_output_channels: max_out,
        max_input_channels: max_in,
        sample_rates: rates,
        default_sample_rate,
        buffer_range,
    }
}

/// Lists all devices of a backend.
pub fn list_devices(backend: Backend) -> Result<Vec<DeviceInfo>> {
    let host = host(backend)?;
    let resolved = Backend::from_host_name(host.id().name()).unwrap_or(backend);
    let def_out = host
        .default_output_device()
        .map(|d| device_name(&d))
        .unwrap_or_default();
    let def_in = host
        .default_input_device()
        .map(|d| device_name(&d))
        .unwrap_or_default();
    Ok(host
        .devices()?
        .map(|d| describe(resolved, &d, &def_out, &def_in))
        .collect())
}

/// Name of the default output device of a backend.
pub fn default_output_device_name(backend: Backend) -> Option<String> {
    host(backend)
        .ok()?
        .default_output_device()
        .map(|d| device_name(&d))
}

/// Finds a device whose name or id contains `pattern` (case-insensitive),
/// preferring exact name matches.
pub(crate) fn find_device(host: &cpal::Host, pattern: &str, output: bool) -> Option<cpal::Device> {
    let p = pattern.to_lowercase();
    let devs: Vec<cpal::Device> = host
        .devices()
        .ok()?
        .filter(|d| {
            if output {
                d.supports_output()
            } else {
                d.supports_input()
            }
        })
        .collect();
    if let Some(d) = devs.iter().find(|d| device_name(d).to_lowercase() == p) {
        return Some(d.clone());
    }
    devs.into_iter().find(|d| {
        device_name(d).to_lowercase().contains(&p)
            || d.id()
                .map(|i| i.to_string().to_lowercase().contains(&p))
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_names_round_trip() {
        for b in [Backend::CoreAudio, Backend::Alsa, Backend::Jack, Backend::PipeWire] {
            assert_eq!(Backend::from_host_name(b.host_name().unwrap()), Some(b));
        }
        assert_eq!(Backend::from_host_name("alsa"), Some(Backend::Alsa), "case-insensitive");
        assert_eq!(Backend::from_host_name("WASAPI"), None);
        assert!(host(Backend::Default).is_ok(), "the platform default host opens");
    }

    /// Linux / Raspberry Pi: ALSA is always there (direct hardware access on
    /// the Pi, also the fallback on the desktop).
    #[cfg(target_os = "linux")]
    #[test]
    fn alsa_host_on_linux() {
        assert!(backends().contains(&Backend::Alsa), "{:?}", backends());
        assert!(host(Backend::Alsa).is_ok());
    }

    /// JACK (and PipeWire's JACK layer) when built with `--features jack`.
    /// Compiled in is checked; whether a server runs depends on the machine.
    #[cfg(all(target_os = "linux", feature = "jack"))]
    #[test]
    fn jack_host_compiled_in() {
        assert!(cpal::ALL_HOSTS.iter().any(|h| h.name().eq_ignore_ascii_case("JACK")));
        assert_eq!(Backend::from_host_name("JACK"), Some(Backend::Jack));
    }

    /// Native PipeWire when built with `--features pipewire`.
    #[cfg(all(target_os = "linux", feature = "pipewire"))]
    #[test]
    fn pipewire_host_compiled_in() {
        assert!(cpal::ALL_HOSTS.iter().any(|h| h.name().eq_ignore_ascii_case("PipeWire")));
        assert_eq!(Backend::from_host_name("PipeWire"), Some(Backend::PipeWire));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn coreaudio_host_on_macos() {
        assert!(backends().contains(&Backend::CoreAudio));
        assert!(host(Backend::CoreAudio).is_ok());
    }

    #[test]
    fn unavailable_backend_is_an_error_not_a_panic() {
        #[cfg(target_os = "macos")]
        assert!(matches!(host(Backend::Alsa), Err(AudioError::BackendUnavailable(Backend::Alsa))));
        #[cfg(target_os = "linux")]
        assert!(matches!(host(Backend::CoreAudio), Err(AudioError::BackendUnavailable(Backend::CoreAudio))));
    }
}
