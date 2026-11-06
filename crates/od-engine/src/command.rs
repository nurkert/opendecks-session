//! Commands sent from control threads (UI, controller) to the real-time
//! processor.
//!
//! Commands are applied at the start of the next processed block, in the
//! order they were sent. Positions are in seconds of track time, knob values
//! are normalised (documented per variant). Commands that refer to a deck or
//! channel beyond the configured count are ignored.

use std::sync::Arc;

use crate::track::{SampleData, TrackData};

/// Top-level command.
#[derive(Debug)]
pub enum Command {
    /// A command for deck `0..decks`.
    Deck(usize, DeckCommand),
    /// A command for mixer channel `0..decks` (channel *n* plays deck *n*).
    Channel(usize, ChannelCommand),
    /// Master section: crossfader, master and headphone levels.
    Mixer(MixerCommand),
    /// A command for Beat FX unit `0..BEAT_FX_UNITS`.
    BeatFx(usize, BeatFxCommand),
    Sampler(SamplerCommand),
    /// Global quantize: snaps cue/loop/hot-cue positions to the beatgrid and
    /// keeps phase on jumps.
    SetQuantize(bool),
    /// Explicitly selects the sync master deck (`None` = automatic: the
    /// first playing deck that is not synced to another one).
    SetMasterDeck(Option<usize>),
    /// Enables/disables the record tap (master stereo into the record ring).
    SetRecordTap(bool),
    /// Internal: attaches an additional command queue (see
    /// [`crate::Engine::new_sender`]).
    #[doc(hidden)]
    AttachSender(rtrb::Consumer<Command>),
}

/// Tempo fader range (± percent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TempoRange {
    Six,
    #[default]
    Ten,
    Sixteen,
    /// ±100 % (wide).
    Wide,
}

impl TempoRange {
    pub fn fraction(self) -> f64 {
        match self {
            TempoRange::Six => 0.06,
            TempoRange::Ten => 0.10,
            TempoRange::Sixteen => 0.16,
            TempoRange::Wide => 1.0,
        }
    }
}

/// What the jog wheel top does when touched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JogMode {
    /// Touching the platter holds the record; turning it scratches.
    #[default]
    Vinyl,
    /// Turning bends the pitch temporarily (CDJ "CDJ mode").
    Bend,
}

/// Direction for memory-cue navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryCueJump {
    Previous,
    Next,
    /// Jumps to the memory cue with this index (sorted by position).
    Index(usize),
}

#[derive(Debug)]
pub enum DeckCommand {
    /// Loads a track (replacing any loaded one). The deck pauses at the
    /// first memory cue, or the first beat/start of the track.
    Load(Arc<TrackData>),
    /// Replaces the loaded track's data with a new build of the *same* track
    /// (same [`TrackData::id`]), e.g. once stems were separated, without
    /// interrupting playback: position, play state, loops, cues, tempo,
    /// sync and keylock are kept. Ignored if another track (or none) is
    /// loaded. Stem levels ramp from unity to their targets (no clicks).
    SwapTrackData(Arc<TrackData>),
    Unload,
    Play,
    Pause,
    TogglePlay,
    /// CDJ cue button. `true` = pressed, `false` = released.
    ///
    /// * Paused away from the cue point: pressing sets the cue point at the
    ///   current (quantized) position and previews from it while held.
    /// * Paused at the cue point: plays while held; release returns to the
    ///   cue point and pauses (unless Play was pressed while holding).
    /// * Playing: returns to the cue point and pauses.
    Cue(bool),
    /// Tempo fader position −1..=1 (0 = original tempo); scaled by the range.
    SetTempoFader(f32),
    SetTempoRange(TempoRange),
    SetKeylock(bool),
    /// Key shift in semitones (only effective with keylock).
    SetKeyShift(f32),
    /// Beat sync to the master deck (BPM and phase).
    SetSync(bool),
    /// Temporary relative speed offset, e.g. ±0.04 while a nudge button is
    /// held; 0.0 to release.
    SetPitchBend(f32),
    SetJogMode(JogMode),
    /// Jog platter touch on/off.
    JogTouch(bool),
    /// Jog rotation in platter revolutions (one revolution = 1.8 s of
    /// audio, like a 33⅓ rpm record). Scratches while touched in vinyl mode,
    /// bends the pitch otherwise; searches while paused.
    JogTurn(f32),
    /// Hot cue pad A–H (`slot` 0..8). Pressing an empty slot stores a hot
    /// cue (a loop hot cue if a loop is active); pressing a stored slot
    /// jumps (quantized), and while paused previews until released.
    HotCue {
        slot: usize,
        pressed: bool,
    },
    /// Stores a hot cue at the current (quantized) position, or the active
    /// loop.
    SetHotCue {
        slot: usize,
    },
    /// Stores a hot cue at an explicit position (e.g. edited in the UI).
    SetHotCueAt {
        slot: usize,
        start: f64,
        loop_end: Option<f64>,
    },
    DeleteHotCue {
        slot: usize,
    },
    /// Jumps to a memory cue and makes it the cue point.
    MemoryCue(MemoryCueJump),
    /// Sets the loop in point at the current (quantized) position.
    LoopIn,
    /// Sets the loop out point and activates the loop.
    LoopOut,
    /// Leaves the active loop (playback continues past loop out).
    LoopExit,
    /// Re-activates the last loop and jumps to its start.
    Reloop,
    /// Activates a loop of `beats` beats (1/32 … 32) starting at the current
    /// beat (quantized) or position.
    AutoLoop {
        beats: f64,
    },
    LoopHalve,
    LoopDouble,
    /// Loop roll: while held, loops `beats` beats; playback continues
    /// underneath and resumes from there on release (like slip).
    LoopRoll {
        beats: f64,
        on: bool,
    },
    /// Slip mode: during loops, scratches, reverse and rolls the playhead
    /// keeps advancing underneath; ending the action resumes from there.
    SetSlip(bool),
    /// Moves the playhead by ±N beats (moves an active loop with it).
    BeatJump {
        beats: f64,
    },
    SetReverse(bool),
    /// Seeks to an absolute position in seconds.
    Seek {
        seconds: f64,
    },
    /// Needle search on the strip: `fraction` 0..=1 of the track length.
    NeedleSearch {
        fraction: f64,
    },
    /// Level 0..=1 of one part of the track (1 = unchanged, 0 = muted).
    /// Ramped over ~20 ms. Only audible if the loaded track has stems
    /// ([`crate::DeckState::stems_available`]); levels persist across loads.
    SetStemLevel {
        stem: crate::stems::Stem,
        level: f32,
    },
}

/// Colour FX (one knob per channel, −1..=1; 0 = off).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFxKind {
    /// Negative = low-pass, positive = high-pass.
    #[default]
    Filter,
    /// Filtered white noise added to the channel.
    Noise,
    /// Reverb send with high/low-passed tail.
    Space,
    /// Bit/sample-rate reduction.
    Crush,
    /// Tempo-synced echo (3/4 beat).
    Echo,
}

/// Crossfader assignment of a channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum XfAssign {
    A,
    B,
    #[default]
    Thru,
}

/// Channel fader response curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FaderCurve {
    /// Linear in amplitude.
    Linear,
    /// Logarithmic feel (amplitude = x²); default.
    #[default]
    Smooth,
    /// Steep: most of the gain in the first quarter (fast mixing).
    Sharp,
}

/// Crossfader response curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossfaderCurve {
    /// Constant power (sin/cos), smooth blends.
    #[default]
    ConstantPower,
    /// Linear crossfade (−6 dB in the middle).
    Linear,
    /// Scratch cut: both sides full except within a few % of the ends.
    Cut,
}

#[derive(Clone, Copy, Debug)]
pub enum ChannelCommand {
    /// Trim gain in dB (−inf..+12 dB knob), added to auto-gain.
    SetTrim(f32),
    /// Applies the track's loudness-derived gain.
    SetAutoGain(bool),
    /// EQ band (0 = low, 1 = mid, 2 = high), knob −1..=1, −1 = kill (isolator).
    SetEq {
        band: usize,
        value: f32,
    },
    SetEqMode(crate::dsp::eq::EqMode),
    SetColorFx(ColorFxKind),
    /// Colour FX knob −1..=1, 0 = bypass.
    SetColorFxAmount(f32),
    /// Channel fader 0..=1.
    SetFader(f32),
    SetFaderCurve(FaderCurve),
    SetCrossfaderAssign(XfAssign),
    /// Headphone cue (PFL) on/off.
    SetCue(bool),
}

#[derive(Clone, Copy, Debug)]
pub enum MixerCommand {
    /// Crossfader −1 (full A) ..= 1 (full B).
    SetCrossfader(f32),
    SetCrossfaderCurve(CrossfaderCurve),
    /// Master level 0..=1 (1 = 0 dB; the knob maps 0.0 → −inf).
    SetMasterLevel(f32),
    /// Headphone mix 0 = cue only ..= 1 = master only.
    SetHeadphoneMix(f32),
    /// Headphone level 0..=1.
    SetHeadphoneLevel(f32),
    /// Sampler output level 0..=1.
    SetSamplerLevel(f32),
    /// Master limiter + soft clip (default on).
    SetLimiter(bool),
    ResetClipIndicators,
}

/// Beat FX type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BeatFxKind {
    #[default]
    Echo,
    /// Single tempo-synced repeat.
    Delay,
    Reverb,
    Flanger,
    Phaser,
    /// Repeats the last `beats` captured when switched on.
    Roll,
    /// Synced low-pass/high-pass sweep.
    FilterSweep,
    /// Synced volume gate.
    Trans,
}

/// Where a Beat FX unit is inserted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FxTarget {
    #[default]
    Master,
    Channel(usize),
}

#[derive(Clone, Copy, Debug)]
pub enum BeatFxCommand {
    SetKind(BeatFxKind),
    SetTarget(FxTarget),
    SetOn(bool),
    /// Wet/dry (depth) 0..=1.
    SetLevel(f32),
    /// Beat fraction: 1/32 … 16 beats.
    SetBeats(f32),
}

/// Sampler slot playback mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SampleMode {
    #[default]
    OneShot,
    Loop,
}

#[derive(Debug)]
pub enum SamplerCommand {
    Load {
        slot: usize,
        sample: Arc<SampleData>,
    },
    Unload {
        slot: usize,
    },
    /// Starts (or restarts) a slot from the beginning.
    Trigger {
        slot: usize,
    },
    Stop {
        slot: usize,
    },
    StopAll,
    SetMode {
        slot: usize,
        mode: SampleMode,
    },
    /// Slot gain 0..=2 (linear).
    SetGain {
        slot: usize,
        gain: f32,
    },
}
