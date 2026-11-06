//! Pioneer DDJ-FLX10 device settings over USB vendor control transfers.
//!
//! The protocol constants in this module were recovered by static analysis
//! (clean room, for interoperability) of the vendor's settings utility. They
//! are **unverified on hardware**; see `docs/ddj-flx10-usb-protocol.md` for the
//! evidence, the confidence of every item and a capture recipe.
//!
//! Shape of the protocol:
//!
//! - Every setting is one control transfer on the default pipe (endpoint 0)
//!   with no data stage: `bmRequestType = 0x40` (vendor, host-to-device,
//!   recipient device), `bRequest = 0x03`, `wIndex` selects the setting and
//!   `wValue` carries the value (for most settings `index << 8`).
//! - Reads use `bmRequestType = 0xC0`, `bRequest = 0x00`, `wValue = 0` and a
//!   `wIndex` register (firmware version is `0xC001`, 2 bytes).
//!
//! [`encode`] is pure and always available. The rusb-backed
//! [`Flx10Device`] is behind the `usb-settings` cargo feature.

/// USB vendor ID of the DDJ-FLX10.
pub const VENDOR_ID: u16 = 0x2B73;
/// USB product ID of the DDJ-FLX10.
pub const PRODUCT_ID: u16 = 0x0041;

/// `bmRequestType` for setting writes: vendor | host-to-device | device.
pub const REQUEST_TYPE_WRITE: u8 = 0x40;
/// `bmRequestType` for reads: vendor | device-to-host | device.
pub const REQUEST_TYPE_READ: u8 = 0xC0;
/// `bRequest` used for every setting write.
pub const REQUEST_WRITE: u8 = 0x03;
/// `bRequest` used for every read.
pub const REQUEST_READ: u8 = 0x00;

/// `wIndex` values ("registers") of setting writes.
pub mod reg {
    /// USB audio input routing (which mixer signal is sent to the host).
    pub const USB_INPUT_AUDIO: u16 = 0x8002;
    /// USB output boost level.
    pub const USB_OUTPUT_LEVEL: u16 = 0x8003;
    /// MIDI channel. Present in the vendor library but never called for the
    /// FLX10 (the public API returns "unsupported"); value encoding unknown.
    pub const MIDI_CHANNEL: u16 = 0x8004;
    /// MIDI key mode (toggle/trigger). Same caveat as [`MIDI_CHANNEL`].
    pub const MIDI_KEY_MODE: u16 = 0x8005;
    pub const FADER_START: u16 = 0xC013;
    pub const REC_OUT: u16 = 0xC01D;
    pub const TIME_MODE: u16 = 0xC026;
    pub const VIRTUAL_DECK_MODE: u16 = 0xC027;
    pub const SLIP_MODE_FLASHING: u16 = 0xC028;
    pub const SLIP_BUTTON_LED: u16 = 0xC029;
    pub const JOG_RING_COLOR: u16 = 0xC02B;
}

/// `wIndex` values and lengths of reads.
pub mod read_reg {
    /// Firmware version, 2 bytes, little-endian u16 (`major << 8 | minor`).
    pub const FIRMWARE_VERSION: (u16, u16) = (0xC001, 2);
    /// Mixer input selector positions, 5 bytes (layout partly unknown).
    pub const INPUT_SELECTOR: (u16, u16) = (0x8002, 5);
    /// "Serato active" flag, 2 bytes, first byte 0/1.
    pub const SERATO_ACTIVE: (u16, u16) = (0x8004, 2);
    /// Port information, 2 bytes, first byte used (meaning unknown).
    pub const PORT_INFO: (u16, u16) = (0x8006, 2);
    /// Certification level, 4 bytes, big-endian u32.
    pub const CERTIFY_LEVEL: (u16, u16) = (0x8007, 4);
}

/// One host-to-device control transfer, ready to submit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlTransfer {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    /// Data stage. Empty for every known setting write (`wLength = 0`).
    pub data: Vec<u8>,
}

/// One device-to-host control transfer description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlRead {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl ControlRead {
    const fn register((index, length): (u16, u16)) -> Self {
        Self { request_type: REQUEST_TYPE_READ, request: REQUEST_READ, value: 0, index, length }
    }
}

/// Read request for the firmware version.
pub const fn firmware_version_read() -> ControlRead {
    ControlRead::register(read_reg::FIRMWARE_VERSION)
}

/// Firmware version as reported by the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FirmwareVersion(pub u16);

impl FirmwareVersion {
    /// Decodes the 2-byte response (little-endian).
    pub fn decode(data: &[u8]) -> Option<Self> {
        match data {
            [lo, hi, ..] => Some(Self(u16::from_le_bytes([*lo, *hi]))),
            _ => None,
        }
    }
}

impl std::fmt::Display for FirmwareVersion {
    /// Formats like the vendor utility: high byte and low byte in hex,
    /// e.g. `0x0102` -> `1.02`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:x}.{:02x}", self.0 >> 8, self.0 & 0xFF)
    }
}

/// Fader start option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaderStart {
    Off = 0,
    /// Fader start on, and SYNC is turned on for the deck when it starts.
    OnWithSync = 1,
    On = 2,
}

/// What the REC OUT signal contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecOut {
    WithMic = 0,
    WithoutMic = 1,
}

/// Time display mode on the jog displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeMode {
    Elapsed = 0,
    Remaining = 1,
}

/// SLIP button LED behaviour while slip mode is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlipButtonLed {
    Lit = 0,
    Blink = 1,
}

/// Jog ring colour mode (the vendor UI only names them MODE 1..3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JogRingColor {
    Mode1 = 0,
    Mode2 = 1,
    Mode3 = 2,
}

/// USB output boost level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoostLevel {
    Plus15dB = 0,
    Plus12dB = 1,
    Plus9dB = 2,
    Plus6dB = 3,
    Plus3dB = 4,
    ZeroDb = 5,
}

/// Mixer channel 1..4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixerChannel {
    Ch1 = 1,
    Ch2 = 2,
    Ch3 = 3,
    Ch4 = 4,
}

/// Physical input type of a channel ("control tone" sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlTone {
    Phono,
    Line,
    Digital,
}

/// Signal routed to a USB audio input stream of the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbAudioSource {
    MixWithMic,
    MixWithoutMic,
    PreFader(MixerChannel),
    PostFader(MixerChannel),
    CrossfaderA,
    CrossfaderB,
    Mic,
    /// Raw channel input. Only valid on the USB stream of the same channel.
    ControlTone(MixerChannel, ControlTone),
}

/// USB input stream selector as used by the vendor library: `0` is the
/// master/record stream, `1..=4` are the per-channel streams.
pub type UsbInputChannel = u8;

/// A validated (USB input stream, source) pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbInputSelection {
    channel: UsbInputChannel,
    source: UsbAudioSource,
    value: u16,
}

impl UsbInputSelection {
    /// Returns `None` if `source` is not selectable on `channel`.
    pub fn new(channel: UsbInputChannel, source: UsbAudioSource) -> Option<Self> {
        usb_input_value(channel, source).map(|value| Self { channel, source, value })
    }

    pub fn channel(&self) -> UsbInputChannel {
        self.channel
    }

    pub fn source(&self) -> UsbAudioSource {
        self.source
    }

    /// The `wValue` sent to the device.
    pub fn wire_value(&self) -> u16 {
        self.value
    }
}

/// Recovered `wValue` for a USB input routing choice.
///
/// High byte = `channel + 1`. Low byte = source code: `0x01` line, `0x02`
/// digital, `0x03` phono, `0x05` pre-fader, `0x06` post-fader, `0x07` cross
/// fader A, `0x08` cross fader B, `0x09` mic, `0x0A` mix with mic, `0x0E` mix
/// without mic. On stream 0 the fader taps name the channel in the high nibble
/// of the low byte (`0x3n` pre-fader, `0x1n` post-fader).
fn usb_input_value(channel: UsbInputChannel, source: UsbAudioSource) -> Option<u16> {
    use UsbAudioSource::*;
    if channel > 4 {
        return None;
    }
    let hi = u16::from(channel + 1) << 8;
    let lo: u16 = match (channel, source) {
        (_, MixWithMic) => 0x0A,
        (_, MixWithoutMic) => 0x0E,
        (_, CrossfaderA) => 0x07,
        (_, CrossfaderB) => 0x08,
        (_, Mic) => 0x09,
        (0, PreFader(ch)) => 0x30 | ch as u16,
        (0, PostFader(ch)) => 0x10 | ch as u16,
        (0, ControlTone(..)) => return None,
        (c, PreFader(ch)) if ch as u8 == c => 0x05,
        (c, PostFader(ch)) if ch as u8 == c => 0x06,
        (c, ControlTone(ch, tone)) if ch as u8 == c => match tone {
            ControlTone::Line => 0x01,
            ControlTone::Digital => 0x02,
            ControlTone::Phono => 0x03,
        },
        _ => return None,
    };
    Some(hi | lo)
}

/// A device setting the FLX10 accepts over USB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flx10Setting {
    UsbInputAudio(UsbInputSelection),
    BoostLevel(BoostLevel),
    FaderStart(FaderStart),
    RecOut(RecOut),
    TimeMode(TimeMode),
    /// Slip mode flashing: enabled / disabled.
    SlipModeFlashing(bool),
    SlipButtonLed(SlipButtonLed),
    /// Virtual deck mode ("display HOT CUE indicator"): on / off.
    VirtualDeckMode(bool),
    JogRingColor(JogRingColor),
    /// Escape hatch for experiments on hardware: write `value` to `index`
    /// with the standard write request.
    Raw { index: u16, value: u16 },
}

const fn hi(v: u8) -> u16 {
    (v as u16) << 8
}

/// Encodes one setting as a control transfer. Pure.
pub fn encode(setting: Flx10Setting) -> ControlTransfer {
    use Flx10Setting::*;
    let (index, value) = match setting {
        UsbInputAudio(sel) => (reg::USB_INPUT_AUDIO, sel.wire_value()),
        BoostLevel(level) => (reg::USB_OUTPUT_LEVEL, hi(level as u8)),
        FaderStart(v) => (reg::FADER_START, hi(v as u8)),
        RecOut(v) => (reg::REC_OUT, hi(v as u8)),
        TimeMode(v) => (reg::TIME_MODE, hi(v as u8)),
        SlipModeFlashing(on) => (reg::SLIP_MODE_FLASHING, hi(on as u8)),
        SlipButtonLed(v) => (reg::SLIP_BUTTON_LED, hi(v as u8)),
        VirtualDeckMode(on) => (reg::VIRTUAL_DECK_MODE, hi(on as u8)),
        JogRingColor(v) => (reg::JOG_RING_COLOR, hi(v as u8)),
        Raw { index, value } => (index, value),
    };
    ControlTransfer {
        request_type: REQUEST_TYPE_WRITE,
        request: REQUEST_WRITE,
        value,
        index,
        data: Vec::new(),
    }
}

/// Values the vendor utility writes when it has no saved preference.
pub fn vendor_defaults() -> [Flx10Setting; 7] {
    [
        Flx10Setting::SlipModeFlashing(true),
        Flx10Setting::SlipButtonLed(SlipButtonLed::Lit),
        Flx10Setting::FaderStart(FaderStart::On),
        Flx10Setting::JogRingColor(JogRingColor::Mode1),
        Flx10Setting::TimeMode(TimeMode::Remaining),
        Flx10Setting::VirtualDeckMode(true),
        Flx10Setting::RecOut(RecOut::WithMic),
    ]
}

#[cfg(feature = "usb-settings")]
pub use device::{Flx10Device, UsbSettingsError};

#[cfg(feature = "usb-settings")]
mod device {
    use super::*;
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_millis(1000);

    #[derive(Debug, thiserror::Error)]
    pub enum UsbSettingsError {
        #[error("no DDJ-FLX10 (USB {VENDOR_ID:04x}:{PRODUCT_ID:04x}) found")]
        NotFound,
        #[error("short read: expected {expected} bytes, got {got}")]
        ShortRead { expected: usize, got: usize },
        #[error("USB error: {0}")]
        Usb(#[from] rusb::Error),
    }

    /// An opened DDJ-FLX10. Only the default control pipe is used, so no
    /// interface is claimed and the class-compliant audio/MIDI drivers keep
    /// working.
    pub struct Flx10Device {
        handle: rusb::DeviceHandle<rusb::GlobalContext>,
    }

    impl Flx10Device {
        /// Opens the first attached DDJ-FLX10.
        pub fn open() -> Result<Self, UsbSettingsError> {
            let handle = rusb::open_device_with_vid_pid(VENDOR_ID, PRODUCT_ID)
                .ok_or(UsbSettingsError::NotFound)?;
            Ok(Self { handle })
        }

        /// Sends the settings in order. Stops at the first failure.
        pub fn apply(&self, settings: &[Flx10Setting]) -> Result<(), UsbSettingsError> {
            for &s in settings {
                let t = encode(s);
                tracing::debug!(?s, value = t.value, index = t.index, "FLX10 setting write");
                self.handle.write_control(
                    t.request_type,
                    t.request,
                    t.value,
                    t.index,
                    &t.data,
                    TIMEOUT,
                )?;
            }
            Ok(())
        }

        /// Executes a read request and returns the bytes received.
        pub fn read(&self, r: ControlRead) -> Result<Vec<u8>, UsbSettingsError> {
            let mut buf = vec![0u8; usize::from(r.length)];
            let n = self
                .handle
                .read_control(r.request_type, r.request, r.value, r.index, &mut buf, TIMEOUT)?;
            buf.truncate(n);
            Ok(buf)
        }

        /// Reads the firmware version.
        pub fn firmware_version(&self) -> Result<FirmwareVersion, UsbSettingsError> {
            let data = self.read(firmware_version_read())?;
            FirmwareVersion::decode(&data)
                .ok_or(UsbSettingsError::ShortRead { expected: 2, got: data.len() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(s: Flx10Setting) -> (u8, u8, u16, u16, usize) {
        let t = encode(s);
        (t.request_type, t.request, t.value, t.index, t.data.len())
    }

    #[test]
    fn simple_settings_match_recovered_constants() {
        use Flx10Setting::*;
        assert_eq!(wire(FaderStart(super::FaderStart::Off)), (0x40, 0x03, 0x0000, 0xC013, 0));
        assert_eq!(wire(FaderStart(super::FaderStart::OnWithSync)), (0x40, 0x03, 0x0100, 0xC013, 0));
        assert_eq!(wire(FaderStart(super::FaderStart::On)), (0x40, 0x03, 0x0200, 0xC013, 0));
        assert_eq!(wire(RecOut(super::RecOut::WithMic)), (0x40, 0x03, 0x0000, 0xC01D, 0));
        assert_eq!(wire(RecOut(super::RecOut::WithoutMic)), (0x40, 0x03, 0x0100, 0xC01D, 0));
        assert_eq!(wire(TimeMode(super::TimeMode::Elapsed)), (0x40, 0x03, 0x0000, 0xC026, 0));
        assert_eq!(wire(TimeMode(super::TimeMode::Remaining)), (0x40, 0x03, 0x0100, 0xC026, 0));
        assert_eq!(wire(VirtualDeckMode(true)), (0x40, 0x03, 0x0100, 0xC027, 0));
        assert_eq!(wire(VirtualDeckMode(false)), (0x40, 0x03, 0x0000, 0xC027, 0));
        assert_eq!(wire(SlipModeFlashing(true)), (0x40, 0x03, 0x0100, 0xC028, 0));
        assert_eq!(wire(SlipModeFlashing(false)), (0x40, 0x03, 0x0000, 0xC028, 0));
        assert_eq!(wire(SlipButtonLed(super::SlipButtonLed::Lit)), (0x40, 0x03, 0x0000, 0xC029, 0));
        assert_eq!(wire(SlipButtonLed(super::SlipButtonLed::Blink)), (0x40, 0x03, 0x0100, 0xC029, 0));
        assert_eq!(wire(JogRingColor(super::JogRingColor::Mode1)), (0x40, 0x03, 0x0000, 0xC02B, 0));
        assert_eq!(wire(JogRingColor(super::JogRingColor::Mode2)), (0x40, 0x03, 0x0100, 0xC02B, 0));
        assert_eq!(wire(JogRingColor(super::JogRingColor::Mode3)), (0x40, 0x03, 0x0200, 0xC02B, 0));
        assert_eq!(wire(Raw { index: 0x1234, value: 0xABCD }), (0x40, 0x03, 0xABCD, 0x1234, 0));
    }

    #[test]
    fn boost_levels() {
        let all = [
            BoostLevel::Plus15dB,
            BoostLevel::Plus12dB,
            BoostLevel::Plus9dB,
            BoostLevel::Plus6dB,
            BoostLevel::Plus3dB,
            BoostLevel::ZeroDb,
        ];
        for (i, b) in all.into_iter().enumerate() {
            assert_eq!(wire(Flx10Setting::BoostLevel(b)), (0x40, 0x03, (i as u16) << 8, 0x8003, 0));
        }
    }

    /// The recovered table, in vendor index order: stream 0 has 13 choices,
    /// streams 1..=4 have 10.
    fn vendor_table() -> Vec<(u8, UsbAudioSource, u16)> {
        use MixerChannel::*;
        use UsbAudioSource::*;
        let mut t = vec![
            (0, MixWithMic, 0x010A),
            (0, PreFader(Ch1), 0x0131),
            (0, PreFader(Ch2), 0x0132),
            (0, PreFader(Ch3), 0x0133),
            (0, PreFader(Ch4), 0x0134),
            (0, PostFader(Ch1), 0x0111),
            (0, PostFader(Ch2), 0x0112),
            (0, PostFader(Ch3), 0x0113),
            (0, PostFader(Ch4), 0x0114),
            (0, CrossfaderA, 0x0107),
            (0, CrossfaderB, 0x0108),
            (0, Mic, 0x0109),
            (0, MixWithoutMic, 0x010E),
        ];
        for (c, ch) in [(1u8, Ch1), (2, Ch2), (3, Ch3), (4, Ch4)] {
            let h = u16::from(c + 1) << 8;
            t.extend([
                (c, ControlTone(ch, super::ControlTone::Phono), h | 0x03),
                (c, ControlTone(ch, super::ControlTone::Line), h | 0x01),
                (c, ControlTone(ch, super::ControlTone::Digital), h | 0x02),
                (c, PreFader(ch), h | 0x05),
                (c, PostFader(ch), h | 0x06),
                (c, CrossfaderA, h | 0x07),
                (c, CrossfaderB, h | 0x08),
                (c, Mic, h | 0x09),
                (c, MixWithMic, h | 0x0A),
                (c, MixWithoutMic, h | 0x0E),
            ]);
        }
        t
    }

    #[test]
    fn usb_input_audio_matches_vendor_table() {
        let table = vendor_table();
        assert_eq!(table.len(), 13 + 4 * 10);
        for (channel, source, value) in table {
            let sel = UsbInputSelection::new(channel, source).expect("valid pair");
            assert_eq!(
                wire(Flx10Setting::UsbInputAudio(sel)),
                (0x40, 0x03, value, 0x8002, 0),
                "channel {channel} source {source:?}"
            );
        }
    }

    #[test]
    fn usb_input_audio_rejects_invalid_pairs() {
        use MixerChannel::*;
        use UsbAudioSource::*;
        assert!(UsbInputSelection::new(5, Mic).is_none());
        assert!(UsbInputSelection::new(0, ControlTone(Ch1, super::ControlTone::Line)).is_none());
        assert!(UsbInputSelection::new(1, PreFader(Ch2)).is_none());
        assert!(UsbInputSelection::new(3, ControlTone(Ch4, super::ControlTone::Phono)).is_none());
    }

    #[test]
    fn firmware_read_and_decode() {
        let r = firmware_version_read();
        assert_eq!(
            r,
            ControlRead { request_type: 0xC0, request: 0x00, value: 0, index: 0xC001, length: 2 }
        );
        let v = FirmwareVersion::decode(&[0x02, 0x01]).unwrap();
        assert_eq!(v, FirmwareVersion(0x0102));
        assert_eq!(v.to_string(), "1.02");
        assert_eq!(FirmwareVersion(0x0110).to_string(), "1.10");
        assert!(FirmwareVersion::decode(&[0x01]).is_none());
    }

    #[test]
    fn vendor_defaults_encode() {
        let d: Vec<_> = vendor_defaults().into_iter().map(|s| (encode(s).index, encode(s).value)).collect();
        assert_eq!(
            d,
            [
                (0xC028, 0x0100),
                (0xC029, 0x0000),
                (0xC013, 0x0200),
                (0xC02B, 0x0000),
                (0xC026, 0x0100),
                (0xC027, 0x0100),
                (0xC01D, 0x0000),
            ]
        );
    }
}
