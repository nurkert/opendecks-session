//! State snapshot published by the processor after every block, and events.
//!
//! The snapshot is a plain `Copy` value with fixed-size arrays (no heap), so
//! publishing it from the audio thread is a memcpy into a triple buffer.

use od_core::TrackId;

use od_core::MusicalKey;

use crate::command::{
    BeatFxKind, ColorFxKind, CrossfaderCurve, FxTarget, JogMode, PadFxPreset, ReleaseFxKind,
    SampleMode, TempoRange, XfAssign,
};
use crate::config::{
    BEAT_FX_SLOTS, BEAT_FX_UNITS, HOT_CUE_BANKS, HOT_CUES, MAX_DECKS, SAMPLER_TOTAL,
};
use crate::dsp::meter::MeterReading;
use crate::track::HotCue;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoopState {
    pub active: bool,
    /// Loop in, seconds (`None` if not set).
    pub start: Option<f64>,
    /// Loop out, seconds.
    pub end: Option<f64>,
    /// Loop length in beats, if it came from an auto loop / roll.
    pub beats: Option<f64>,
    pub roll: bool,
}

/// Slicer pad mode state (see [`crate::SlicerCommand`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SlicerState {
    pub active: bool,
    pub loop_mode: bool,
    /// Domain length in beats.
    pub beats: f64,
    /// Domain start/end, seconds.
    pub start: f64,
    pub end: f64,
    /// Slice (0..8) the (slip) playhead is in.
    pub current: u8,
    /// Slice pad being held.
    pub held: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeckState {
    pub track: Option<TrackId>,
    /// Track duration in seconds.
    pub duration: f64,
    /// Playhead in seconds (what is audible now, before output latency).
    pub position: f64,
    pub playing: bool,
    /// Current playback speed (1.0 = original, negative in reverse/scratch).
    pub speed: f64,
    /// Tempo factor from fader or sync (1.0 = original tempo).
    pub tempo: f64,
    pub tempo_fader: f32,
    pub tempo_range: TempoRange,
    /// Track BPM at the playhead (from the grid), 0 without grid.
    pub track_bpm: f64,
    /// Effective BPM (track BPM × tempo).
    pub bpm: f64,
    /// Fractional beat number at the playhead (grid beat index).
    pub beat: f64,
    /// Bar position of the current beat 1..=4, 0 without grid.
    pub beat_in_bar: u8,
    pub keylock: bool,
    /// Key shift in semitones (set directly, by keyboard pads or key sync).
    pub key_shift: f32,
    pub key_sync: bool,
    /// Key of the loaded track as set by [`crate::DeckCommand::SetTrackKey`].
    pub track_key: Option<MusicalKey>,
    /// Audible pitch offset in semitones relative to the track: key shift,
    /// plus the tempo pitch change when keylock is off.
    pub pitch_semitones: f32,
    pub sync: bool,
    pub is_master: bool,
    pub slip: bool,
    /// Where playback resumes when the slip action ends (seconds).
    pub slip_position: Option<f64>,
    pub reverse: bool,
    /// Censor is held (reverse with slip).
    pub censor: bool,
    pub scratching: bool,
    pub jog_mode: JogMode,
    pub cue_point: f64,
    pub auto_cue: bool,
    /// Hot cues A–H of the active bank (`hot_cue_banks[hot_cue_bank]`).
    pub hot_cues: [Option<HotCue>; HOT_CUES],
    pub hot_cue_bank: usize,
    pub hot_cue_banks: [[Option<HotCue>; HOT_CUES]; HOT_CUE_BANKS],
    pub loop_state: LoopState,
    /// Armed active loop (seconds), engaged when the playhead reaches it.
    pub active_loop: Option<(f64, f64)>,
    /// Beat jump size of [`crate::DeckCommand::BeatJumpStep`].
    pub beat_jump_size: f64,
    pub slicer: SlicerState,
    pub at_end: bool,
    /// The loaded track carries separated stems.
    pub stems_available: bool,
    /// Target stem levels, indexed by [`crate::Stem::index`]
    /// (vocals, drums, bass, inst).
    pub stem_levels: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChannelState {
    /// Pre-fader (post trim/EQ/colour FX) meter.
    pub meter: MeterReading,
    pub fader: f32,
    pub trim_db: f32,
    pub auto_gain: bool,
    pub eq: [f32; 3],
    pub color_fx: ColorFxKind,
    pub color_fx_amount: f32,
    pub xf_assign: XfAssign,
    pub cue: bool,
    /// Pad FX of this channel.
    pub pad_fx: PadFxState,
    /// Linear gain from this channel to the master bus (fader × crossfader ×
    /// master level), used by the audible-track logic.
    pub gain_to_master: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BeatFxState {
    pub kind: BeatFxKind,
    pub target: FxTarget,
    pub on: bool,
    pub level: f32,
    pub beats: f32,
    /// Parameter of the current kind.
    pub param: f32,
    pub tap_bpm: Option<f32>,
    /// BPM the unit is currently synced to.
    pub bpm: f32,
    /// Engaged release FX.
    pub release: Option<ReleaseFxKind>,
    /// Still producing sound after being switched off (tail / release).
    pub ringing: bool,
}

/// Pad FX state of one channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PadFxState {
    /// A pad is held.
    pub held: bool,
    /// The preset of the held (or last) pad.
    pub preset: Option<PadFxPreset>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SamplerSlotState {
    pub loaded: bool,
    pub playing: bool,
    /// Triggered, waiting for the quantize boundary.
    pub pending: bool,
    pub mode: SampleMode,
    pub gain: f32,
    /// Playback progress 0..1.
    pub progress: f32,
    pub bpm: Option<f64>,
    pub sync: bool,
    /// Sequencer pattern (bit `n` = step `n`).
    pub pattern: u16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SequencerState {
    pub playing: bool,
    pub recording: bool,
    /// Current step 0..16 (from the master beat clock).
    pub step: u8,
    /// Sampler trigger quantize in beats.
    pub quantize: Option<f64>,
}

/// Microphone / aux input channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputState {
    /// A capture stream is attached.
    pub attached: bool,
    pub on: bool,
    pub level: f32,
    pub cue: bool,
    pub talkover: bool,
    pub talkover_db: f32,
    /// The music is being ducked right now.
    pub ducking: bool,
    pub meter: MeterReading,
    /// Blocks in which the capture ring ran short.
    pub underruns: u64,
}

/// Audible-track information for automatic tracklists (SPEC §11).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudibleState {
    /// Smoothed RMS each deck contributes to the master output.
    pub level: [f32; MAX_DECKS],
    /// Deck is playing a track and contributes more than −40 dBFS RMS.
    pub audible: [bool; MAX_DECKS],
    /// The deck contributing most to the master, if any is audible.
    pub dominant: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineState {
    pub sample_rate: u32,
    pub decks_count: usize,
    /// Frames rendered since the engine started (sample clock).
    pub frames: u64,
    pub decks: [DeckState; MAX_DECKS],
    pub channels: [ChannelState; MAX_DECKS],
    pub master_meter: MeterReading,
    pub headphone_meter: MeterReading,
    pub limiter_reduction_db: f32,
    pub crossfader: f32,
    pub crossfader_curve: CrossfaderCurve,
    pub master_level: f32,
    pub headphone_mix: f32,
    pub headphone_level: f32,
    pub sampler_level: f32,
    pub sampler_cue: bool,
    pub booth_level: f32,
    pub master_cue: bool,
    pub quantize: bool,
    /// Current sync master deck.
    pub master_deck: Option<usize>,
    /// Tempo of the sync master (or the last known), BPM.
    pub master_bpm: f64,
    pub beat_fx: [BeatFxState; BEAT_FX_UNITS],
    /// Slots 1 and 2 of each unit (multi mode).
    pub beat_fx_slots: [[BeatFxState; BEAT_FX_SLOTS - 1]; BEAT_FX_UNITS],
    pub beat_fx_multi: [bool; BEAT_FX_UNITS],
    /// All sampler slots, `bank * SAMPLER_SLOTS + pad`.
    pub sampler: [SamplerSlotState; SAMPLER_TOTAL],
    pub sequencer: SequencerState,
    pub audible: AudibleState,
    pub input: InputState,
    pub record_tap: bool,
    /// Samples dropped because the record ring was full.
    pub record_dropped: u64,
    /// Released objects leaked because the garbage queue was full (diagnostic, expected 0).
    pub garbage_leaked: u64,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            sample_rate: 48_000,
            decks_count: MAX_DECKS,
            frames: 0,
            decks: [DeckState::default(); MAX_DECKS],
            channels: [ChannelState::default(); MAX_DECKS],
            master_meter: MeterReading::default(),
            headphone_meter: MeterReading::default(),
            limiter_reduction_db: 0.0,
            crossfader: 0.0,
            crossfader_curve: CrossfaderCurve::default(),
            master_level: 1.0,
            headphone_mix: 0.0,
            headphone_level: 1.0,
            sampler_level: 1.0,
            sampler_cue: false,
            booth_level: 1.0,
            master_cue: false,
            quantize: true,
            master_deck: None,
            master_bpm: 120.0,
            beat_fx: [BeatFxState::default(); BEAT_FX_UNITS],
            beat_fx_slots: [[BeatFxState::default(); BEAT_FX_SLOTS - 1]; BEAT_FX_UNITS],
            beat_fx_multi: [false; BEAT_FX_UNITS],
            sampler: [SamplerSlotState::default(); SAMPLER_TOTAL],
            sequencer: SequencerState::default(),
            audible: AudibleState::default(),
            input: InputState::default(),
            record_tap: false,
            record_dropped: 0,
            garbage_leaked: 0,
        }
    }
}

/// Discrete events from the processor, e.g. for persisting cues.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    TrackLoaded {
        deck: usize,
        track: TrackId,
    },
    TrackUnloaded {
        deck: usize,
    },
    /// The playhead reached the end of the track and playback stopped.
    TrackEnded {
        deck: usize,
    },
    CuePointSet {
        deck: usize,
        track: TrackId,
        position: f64,
    },
    /// `slot` 0..8 within `bank` (library hot cue index `bank * 8 + slot`).
    HotCueSet {
        deck: usize,
        track: TrackId,
        bank: usize,
        slot: usize,
        cue: HotCue,
    },
    HotCueDeleted {
        deck: usize,
        track: TrackId,
        bank: usize,
        slot: usize,
    },
    MasterDeckChanged {
        deck: Option<usize>,
    },
}
