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
    /// Effect slot 1 or 2 of a Beat FX unit in multi mode (slot 0 is
    /// addressed by [`Command::BeatFx`]). The target follows slot 0.
    BeatFxSlot { unit: usize, slot: usize, cmd: BeatFxCommand },
    /// Multi mode of a Beat FX unit: three effects in a chain.
    SetBeatFxMulti(usize, bool),
    /// Pad FX of mixer channel `0..decks` (an FX unit of its own per
    /// channel, independent of the two Beat FX units).
    PadFx(usize, PadFxCommand),
    /// Sampler slots (`0..SAMPLER_TOTAL`) and step sequencer.
    Sampler(SamplerCommand),
    /// Global quantize: snaps cue/loop/hot-cue positions to the beatgrid and
    /// keeps phase on jumps.
    SetQuantize(bool),
    /// Explicitly selects the sync master deck (`None` = automatic: the
    /// first playing deck that is not synced to another one).
    SetMasterDeck(Option<usize>),
    /// Enables/disables the record tap (master stereo into the record ring).
    SetRecordTap(bool),
    /// Microphone / aux input channel.
    Input(InputCommand),
    /// Internal: attaches an additional command queue (see
    /// [`crate::Engine::new_sender`]).
    #[doc(hidden)]
    AttachSender(rtrb::Consumer<Command>),
}

/// Microphone / aux input (see [`crate::InputState`]).
pub enum InputCommand {
    /// Attaches a capture ring of interleaved samples with `channels`
    /// channels at the engine sample rate (replaces a previous one).
    Attach(rtrb::Consumer<f32>, u16),
    Detach,
    SetOn(bool),
    /// Level 0..=1.5 (square law, 1 = 0 dB).
    SetLevel(f32),
    /// Input to the headphone cue bus.
    SetCue(bool),
    /// Duck the music while the input carries signal.
    SetTalkover(bool),
    /// Music level while ducked, dB (−40..=0).
    SetTalkoverDepth(f32),
}

impl std::fmt::Debug for InputCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InputCommand::Attach(_, ch) => write!(f, "Attach({ch} ch)"),
            InputCommand::Detach => write!(f, "Detach"),
            InputCommand::SetOn(v) => write!(f, "SetOn({v})"),
            InputCommand::SetLevel(v) => write!(f, "SetLevel({v})"),
            InputCommand::SetCue(v) => write!(f, "SetCue({v})"),
            InputCommand::SetTalkover(v) => write!(f, "SetTalkover({v})"),
            InputCommand::SetTalkoverDepth(v) => write!(f, "SetTalkoverDepth({v})"),
        }
    }
}

/// What beat sync matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncMode {
    /// Tempo and beat phase.
    #[default]
    Beat,
    /// Tempo only; the DJ keeps the phase by hand.
    Tempo,
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

/// Slicer pad mode: a domain of `beats` beats (aligned to the beatgrid) is
/// split into [`crate::SLICER_SLICES`] slices, one per pad.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SlicerCommand {
    /// Enables the slicer with the domain (a multiple of `beats` on the
    /// grid, 1 … 64 beats) that contains the playhead. Without `loop_mode`
    /// the domain moves on with playback; with `loop_mode` it is looped.
    Enable { beats: f64, loop_mode: bool },
    /// Disables the slicer (releases a held slice, ends the domain loop).
    Disable,
    /// Slice pad `slice` (0..8). Press: with quantize the playhead jumps to
    /// the same phase within the slice (the beat stays on the grid), without
    /// quantize to the slice start; the slice repeats while held. Release
    /// resumes where normal playback would be (slip).
    Pad { slice: usize, pressed: bool },
}

#[derive(Debug)]
pub enum DeckCommand {
    /// Loads a track (replacing any loaded one) and pauses. With auto cue
    /// on (default) the deck is cued to the first memory cue, else to the
    /// first audible sound ([`TrackData::first_sound`]); with auto cue off
    /// to the start of the track. The hot cue bank, the track key
    /// ([`DeckCommand::SetTrackKey`]) and an armed active loop are reset.
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
    /// Key shift in semitones (−12..=12, fractional allowed). Works with and
    /// without keylock: a non-zero shift routes the deck through the Rubber
    /// Band stretcher (without keylock the tempo pitch change is kept and the
    /// shift is added on top). Setting it switches key sync off.
    SetKeyShift(f32),
    /// Key sync: continuously sets the key shift so that the deck's key
    /// (incl. the tempo pitch change without keylock) matches the sync
    /// master's key or its relative major/minor with the smallest shift
    /// (−6..=6 semitones). Needs [`DeckCommand::SetTrackKey`] on both decks;
    /// no effect on the master deck itself.
    SetKeySync(bool),
    /// Musical key of the loaded track (from analysis/library), used by key
    /// sync. Reset to `None` by `Load`.
    SetTrackKey(Option<od_core::MusicalKey>),
    /// Beat sync to the master deck (BPM and phase, see [`SyncMode`]).
    SetSync(bool),
    /// Tempo-only or beat sync.
    SetSyncMode(SyncMode),
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
    /// Selects the active hot cue bank `0..HOT_CUE_BANKS`. All hot cue
    /// commands (`slot` 0..8) address the active bank.
    SetHotCueBank(usize),
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
    /// Recalls a saved loop (a memory cue with a loop end): previous/next
    /// relative to the playhead, or by index among the saved loops. Jumps to
    /// its start (quantized while playing) and activates it.
    SavedLoop(MemoryCueJump),
    /// Arms an *active loop* (seconds, `start < end`): when the playhead
    /// reaches `start` while playing forward, the loop engages (sample
    /// accurate). `None` disarms. Reset by `Load`.
    SetActiveLoop(Option<(f64, f64)>),
    /// Sets the loop in point at the current (quantized) position.
    LoopIn,
    /// Sets the loop out point and activates the loop.
    LoopOut,
    /// Leaves the active loop (playback continues past loop out).
    LoopExit,
    /// Re-activates the last loop and jumps to its start.
    Reloop,
    /// Activates a loop of `beats` beats (1/32 … 512) starting at the current
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
    /// Beat jump size in beats for [`DeckCommand::BeatJumpStep`]
    /// (1/32 … 512, default 4).
    SetBeatJumpSize(f64),
    /// Beat jump by the configured size, forward or back.
    BeatJumpStep {
        forward: bool,
    },
    SetReverse(bool),
    /// Censor: plays in reverse while held (`true`) and resumes on release
    /// where normal playback would be (slip), whether or not slip is on.
    Censor(bool),
    /// Active censor: censors for `beats` beats of the deck's tempo, then
    /// returns where normal playback would be (slip).
    CensorFor { beats: f64 },
    /// SEARCH buttons (FF / REW): while non-zero the playhead runs at this
    /// many times real time (positive forward, negative backward), audibly,
    /// whether or not the deck plays; 0 releases. Keylock is bypassed.
    Search(f64),
    /// Auto cue on load (default on), see [`DeckCommand::Load`]. Kept across
    /// loads.
    SetAutoCue(bool),
    /// Seeks to an absolute position in seconds.
    Seek {
        seconds: f64,
    },
    /// Needle search on the strip: `fraction` 0..=1 of the track length.
    NeedleSearch {
        fraction: f64,
    },
    /// Slicer pad mode, see [`SlicerCommand`].
    Slicer(SlicerCommand),
    /// Keyboard pad mode: plays hot cue `slot` (active bank) at `semitones`
    /// key shift. Press sets the key shift (key sync off) and jumps to the
    /// hot cue at once (no quantize wait); while paused it previews until
    /// released, like a hot cue pad. The shift stays after release.
    Keyboard {
        slot: usize,
        semitones: f32,
        pressed: bool,
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
    /// Tempo-synced echo (3/4 beat), filtered by knob direction; tails ring
    /// out when the knob returns to the centre.
    Echo,
    /// Pitch shift: left lowers, right raises (±12 semitones at the ends).
    Pitch,
    /// Dub echo: 1/2-beat echo with long, saturating, filtered feedback;
    /// tails ring out.
    DubEcho,
    /// Left: beat-synced 1/16 gate (depth = knob); right: band-pass sweep
    /// rising with the knob.
    Sweep,
    /// Smart CFX: one knob for a combination — left a low-pass with a
    /// 3/4-beat echo, right a high-pass with a reverb, both growing with
    /// the turn; tails ring out.
    Smart,
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
    /// Sampler to the headphone cue bus.
    SetSamplerCue(bool),
    /// Booth output level 0..=1 (square law, 1 = 0 dB). The booth output is
    /// the master bus after Beat FX, before master level and limiter,
    /// soft-clipped; see [`crate::Outputs::booth`].
    SetBoothLevel(f32),
    /// Master cue: adds the master output to the headphone cue bus.
    SetMasterCue(bool),
    /// Master limiter + soft clip (default on).
    SetLimiter(bool),
    ResetClipIndicators,
}

/// Beat FX type. Every effect is synced to the BPM and beat of its unit's
/// clock: the targeted channel's deck (if it plays a track with a grid),
/// else the master clock, or a tapped BPM ([`BeatFxCommand::SetTapBpm`]).
/// `beats` is the effect length, `level` the depth / wet amount and `param`
/// (−1..=1, remembered per kind) is documented per variant. Kinds marked
/// *tail* keep ringing out after being switched off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BeatFxKind {
    /// Echo with feedback, a repeat every `beats`. Tail.
    #[default]
    Echo,
    /// Single repeat after `beats`. Tail.
    Delay,
    /// Echo alternating left/right every `beats`. Tail.
    PingPong,
    /// Reverb; `beats` sets the size; `param` > 0 brightens, < 0 darkens.
    /// Tail.
    Reverb,
    /// Flanger, one LFO cycle per `beats`.
    Flanger,
    /// Phaser, one LFO cycle per `beats`.
    Phaser,
    /// Repeats the `beats` captured from switching on; changing `beats`
    /// while on changes the repeat length within the capture.
    Roll,
    /// Like roll, but changing `beats` while on re-captures from the live
    /// signal (playback has continued underneath).
    SlipRoll,
    /// Synced filter sweep, one cycle per `beats`; `param` ≤ 0 low-pass,
    /// > 0 high-pass.
    Filter,
    /// Synced volume gate (open for the first half of each `beats`).
    Trans,
    /// Pitch shift by `param × 12` semitones (default +6).
    Pitch,
    /// Slows the signal to a stop over `beats` beats when switched on.
    VinylBrake,
    /// Echo whose repeats are pitch-shifted by `param × 12` semitones each
    /// (default +3), spiralling up or down. Tail.
    Helix,
    /// Long diffuse echo; changing `beats` glides the delay time (pitch
    /// sweep). Tail.
    Spiral,
}

impl BeatFxKind {
    pub const ALL: [BeatFxKind; 14] = [
        BeatFxKind::Echo,
        BeatFxKind::Delay,
        BeatFxKind::PingPong,
        BeatFxKind::Reverb,
        BeatFxKind::Flanger,
        BeatFxKind::Phaser,
        BeatFxKind::Roll,
        BeatFxKind::SlipRoll,
        BeatFxKind::Filter,
        BeatFxKind::Trans,
        BeatFxKind::Pitch,
        BeatFxKind::VinylBrake,
        BeatFxKind::Helix,
        BeatFxKind::Spiral,
    ];

    /// Index into [`BeatFxKind::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Whether the effect keeps ringing out after being switched off.
    pub const fn has_tail(self) -> bool {
        matches!(
            self,
            BeatFxKind::Echo
                | BeatFxKind::Delay
                | BeatFxKind::PingPong
                | BeatFxKind::Reverb
                | BeatFxKind::Helix
                | BeatFxKind::Spiral
        )
    }

    /// Default `param` of the kind.
    pub const fn default_param(self) -> f32 {
        match self {
            BeatFxKind::Pitch => 0.5,
            BeatFxKind::Helix => 0.25,
            _ => 0.0,
        }
    }
}

/// Release FX: ends the sound of an FX unit's target musically while
/// engaged. Engaging switches the unit's Beat FX off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseFxKind {
    /// Slows to a stop over `beats`.
    VinylBrake,
    /// Spins backwards and stops over `beats`.
    Backspin,
    /// Cuts the input and echoes the last `beats` (at most 1) out.
    EchoOut,
}

/// Where a Beat FX unit is inserted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FxTarget {
    #[default]
    Master,
    Channel(usize),
    /// The sampler output (before it joins the master).
    Sampler,
    /// Stem FX: one part of channel `n`'s deck (the rest stays dry).
    Part(usize, crate::stems::Stem),
}

#[derive(Clone, Copy, Debug)]
pub enum BeatFxCommand {
    SetKind(BeatFxKind),
    SetTarget(FxTarget),
    SetOn(bool),
    /// Wet/dry (depth) 0..=1, ramped.
    SetLevel(f32),
    /// Beat fraction: 1/32 … 16 beats.
    SetBeats(f32),
    /// Effect parameter −1..=1 of the current kind (see [`BeatFxKind`]).
    SetParam(f32),
    /// Tapped BPM (`Some`, 40..=300) overrides the synced clock; `None`
    /// returns to it.
    SetTapBpm(Option<f32>),
    /// Release FX on the unit's target while `on`, lasting `beats`.
    /// Engaging switches the Beat FX off; releasing brings the dry signal
    /// back (click-free).
    Release {
        kind: ReleaseFxKind,
        beats: f32,
        on: bool,
    },
}

/// What a Pad FX pad does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadFxEffect {
    Beat(BeatFxKind),
    Release(ReleaseFxKind),
}

/// A Pad FX preset: one pad of the Pad FX pad mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadFxPreset {
    pub effect: PadFxEffect,
    /// Effect length in beats (1/32 … 16).
    pub beats: f32,
    /// Depth 0..=1.
    pub level: f32,
    /// Parameter −1..=1 (see [`BeatFxKind`]).
    pub param: f32,
}

/// Pad FX of a channel: momentary effects while a pad is held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadFxCommand {
    /// Pad pressed: engages the preset on the channel. A Beat FX preset
    /// replaces an engaged one; a release preset releases an engaged Beat FX
    /// preset through the release FX.
    Press(PadFxPreset),
    /// Pad released: the effect switches off (tails ring out); a release FX
    /// returns to the dry signal.
    Release,
}

/// Sampler slot playback mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SampleMode {
    /// Plays to the end once.
    #[default]
    OneShot,
    /// Repeats until stopped.
    Loop,
    /// Plays (looping) only while the pad is held: stops on `Release`.
    Gate,
}

/// Sampler commands. `slot` is `0..SAMPLER_TOTAL` (`bank * 16 + pad`);
/// commands with an out-of-range slot are ignored.
#[derive(Debug)]
pub enum SamplerCommand {
    Load {
        slot: usize,
        sample: Arc<SampleData>,
    },
    Unload {
        slot: usize,
    },
    /// Starts (or restarts, crossfading the old voice out) a slot from the
    /// beginning; with sampler quantize at the next quantize boundary of the
    /// master beat clock. While the sequencer records, also stores the
    /// nearest step.
    Trigger {
        slot: usize,
    },
    /// Pad released: stops a slot in `Gate` mode (other modes ignore it).
    Release {
        slot: usize,
    },
    /// Stops a slot (short fade).
    Stop {
        slot: usize,
    },
    StopAll,
    SetMode {
        slot: usize,
        mode: SampleMode,
    },
    /// Slot gain 0..=2 (linear), ramped.
    SetGain {
        slot: usize,
        gain: f32,
    },
    /// Original tempo of the sample (needed for sync).
    SetBpm {
        slot: usize,
        bpm: Option<f64>,
    },
    /// Sync the slot to the master tempo: plays at `master_bpm / bpm` speed
    /// (resampled; pitch follows, like a turntable).
    SetSync {
        slot: usize,
        on: bool,
    },
    /// Trigger quantize in beats of the master clock (e.g. `Some(1.0)`);
    /// `None` = immediate (default).
    SetQuantize(Option<f64>),
    /// Sets/clears step `0..SEQ_STEPS` of a slot's sequencer pattern.
    SeqStep {
        slot: usize,
        step: usize,
        on: bool,
    },
    /// Replaces a slot's pattern (bit `n` = step `n`), e.g. to load a saved
    /// sequence.
    SeqPattern {
        slot: usize,
        mask: u16,
    },
    /// Clears all patterns.
    SeqClear,
    /// Starts/stops the step sequencer. Steps are 16th notes locked to the
    /// master beat clock (step = ⌊beat × 4⌋ mod 16), triggered sample
    /// accurately.
    SeqPlay(bool),
    /// Pattern recording: while on and playing, `Trigger` also stores the
    /// nearest step for the slot.
    SeqRecord(bool),
}
