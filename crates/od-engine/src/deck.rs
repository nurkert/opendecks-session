//! One deck: transport, cueing, loops, slip, sync, jog and the playback
//! resampler / keylock.
//!
//! All positions inside the deck are in frames at the engine rate (`f64`);
//! cues and the beatgrid are converted from seconds on the fly.

use std::sync::Arc;

use od_core::{Mode, MusicalKey};

use crate::command::{DeckCommand, JogMode, MemoryCueJump, SlicerCommand, TempoRange};
use crate::config::{HOT_CUE_BANKS, HOT_CUES, Profile, SLICER_SLICES};
use crate::keylock::{MAX_FEED, Stretcher};
use crate::rt::RtCtx;
use crate::state::{DeckState, Event, LoopState, SlicerState};
use crate::stems::{StemRamp, frame_at};
use crate::track::{HotCue, TrackData};

/// Audio length of one jog platter revolution (33⅓ rpm record).
pub const SECONDS_PER_REVOLUTION: f64 = 1.8;
/// Time constant with which scratch movement follows the jog.
const SCRATCH_SMOOTHING_S: f64 = 0.012;
/// Maximum continuous phase-correction speed offset while synced.
const MAX_PHASE_CORRECTION: f64 = 0.03;

/// Sync master information handed to followers before each block.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SyncTarget {
    pub bpm: f64,
    /// Master beat position (fractional) at the start of the block.
    pub beat: f64,
    pub playing: bool,
    /// Master's sounding key as a major-equivalent pitch class in semitones
    /// (fractional), for key sync.
    pub key: Option<f64>,
}

/// Major-equivalent tonic (relative major for minor keys), 0..12.
fn major_tonic(k: MusicalKey) -> f64 {
    match k.mode {
        Mode::Major => k.tonic as f64,
        Mode::Minor => ((k.tonic + 3) % 12) as f64,
    }
}

/// Slicer domain.
#[derive(Clone, Copy, Debug)]
struct Slicer {
    beats: f64,
    loop_mode: bool,
    /// Domain start as a grid beat (`None` without a grid).
    start_beat: Option<f64>,
    start: f64,
    end: f64,
    held: Option<usize>,
}

/// A jump scheduled for an exact frame offset (quantized hot cue jumps).
#[derive(Clone, Copy, Debug)]
struct PendingJump {
    /// Frames of output until the jump happens.
    in_frames: usize,
    target: f64,
    loop_range: Option<(f64, f64)>,
}

pub(crate) struct Deck {
    index: usize,
    sr: f64,
    track: Option<Arc<TrackData>>,
    pos: f64,
    playing: bool,
    at_end: bool,

    tempo_fader: f32,
    tempo_range: TempoRange,
    sync: bool,
    /// Tempo factor imposed by sync while following a master.
    sync_tempo: Option<f64>,
    sync_snap_pending: bool,
    phase_correction: f64,
    bend: f64,
    jog_bend: f64,
    jog_bend_accum: f64,
    jog_mode: JogMode,
    jog_touched: bool,
    jog_pending: f64,
    scratch_speed: f64,
    reverse: bool,
    speed: f64,

    keylock: bool,
    key_shift: f32,
    key_sync: bool,
    track_key: Option<MusicalKey>,
    stretcher: Option<Stretcher>,
    kl_active: bool,
    /// The stretcher is being primed over several blocks; output comes from
    /// the resampler until the playhead reaches `kl_target`.
    kl_priming: bool,
    /// Input frame at which the primed stretcher's output starts.
    kl_target: f64,
    kl_feed_pos: i64,
    kl_discard: usize,
    kl_in: [Vec<f32>; 2],
    kl_out: [Vec<f32>; 2],
    /// Resampler output used to crossfade into the stretcher output.
    kl_mix: Vec<f32>,

    cue_point: f64,
    cue_preview: bool,
    play_latched: bool,
    hot_cues: [[Option<HotCue>; HOT_CUES]; HOT_CUE_BANKS],
    hot_bank: usize,
    hot_preview: Option<usize>,
    auto_cue: bool,
    beat_jump_size: f64,
    censor: bool,
    /// Armed active loop (frames).
    active_loop: Option<(f64, f64)>,
    slicer: Option<Slicer>,

    loop_in: Option<f64>,
    loop_out: Option<f64>,
    loop_active: bool,
    loop_beats: Option<f64>,
    roll: bool,
    roll_saved: Option<(Option<f64>, Option<f64>, bool, Option<f64>)>,

    slip: bool,
    slip_pos: f64,
    slip_diverged: bool,

    pending_jump: Option<PendingJump>,

    /// Stem levels set by the user (vocals, drums, bass, inst).
    stem_target: [f32; 4],
    /// Smoothed levels applied to output frames (resampler path).
    stem_play: StemRamp,
    /// Smoothed levels applied to the frames fed to the keylock stretcher.
    stem_feed: StemRamp,
}

#[inline]
fn hermite(xm1: f32, x0: f32, x1: f32, x2: f32, t: f32) -> f32 {
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + x0
}

impl Deck {
    pub fn new(index: usize, sample_rate: u32, max_block: usize, profile: Profile) -> Self {
        let stretcher = Stretcher::new(sample_rate, profile);
        Self {
            index,
            sr: sample_rate as f64,
            track: None,
            pos: 0.0,
            playing: false,
            at_end: false,
            tempo_fader: 0.0,
            tempo_range: TempoRange::default(),
            sync: false,
            sync_tempo: None,
            sync_snap_pending: false,
            phase_correction: 0.0,
            bend: 0.0,
            jog_bend: 0.0,
            jog_bend_accum: 0.0,
            jog_mode: JogMode::default(),
            jog_touched: false,
            jog_pending: 0.0,
            scratch_speed: 0.0,
            reverse: false,
            speed: 0.0,
            keylock: false,
            key_shift: 0.0,
            key_sync: false,
            track_key: None,
            stretcher,
            kl_active: false,
            kl_priming: false,
            kl_target: 0.0,
            kl_mix: vec![0.0; MAX_FEED * 2],
            kl_feed_pos: 0,
            kl_discard: 0,
            kl_in: [vec![0.0; MAX_FEED], vec![0.0; MAX_FEED]],
            kl_out: [
                vec![0.0; max_block.max(MAX_FEED)],
                vec![0.0; max_block.max(MAX_FEED)],
            ],
            cue_point: 0.0,
            cue_preview: false,
            play_latched: false,
            hot_cues: [[None; HOT_CUES]; HOT_CUE_BANKS],
            hot_bank: 0,
            hot_preview: None,
            auto_cue: true,
            beat_jump_size: 4.0,
            censor: false,
            active_loop: None,
            slicer: None,
            loop_in: None,
            loop_out: None,
            loop_active: false,
            loop_beats: None,
            roll: false,
            roll_saved: None,
            slip: false,
            slip_pos: 0.0,
            slip_diverged: false,
            pending_jump: None,
            stem_target: [1.0; 4],
            stem_play: StemRamp::new(sample_rate),
            stem_feed: StemRamp::new(sample_rate),
        }
    }

    // ----- helpers -------------------------------------------------------

    fn frames(&self) -> f64 {
        self.track.as_ref().map_or(0.0, |t| t.frames() as f64)
    }

    fn has_grid(&self) -> bool {
        self.track.as_ref().is_some_and(|t| !t.beatgrid.is_empty())
    }

    /// Fractional beat number at a frame position.
    pub fn beat_at(&self, frames: f64) -> Option<f64> {
        self.track
            .as_ref()?
            .beatgrid
            .beat_position(frames / self.sr)
    }

    /// Frame position of a fractional beat number.
    fn frames_of_beat(&self, beat: f64) -> Option<f64> {
        self.track
            .as_ref()?
            .beatgrid
            .time_of_beat(beat)
            .map(|t| t * self.sr)
    }

    /// Track BPM (grid) at the playhead, measured from neighbouring beats.
    pub fn track_bpm(&self) -> Option<f64> {
        let b = self.beat_at(self.pos)?;
        let base = b.floor();
        let t0 = self.frames_of_beat(base)?;
        let t1 = self.frames_of_beat(base + 1.0)?;
        let period = (t1 - t0) / self.sr;
        (period > 1e-6).then(|| 60.0 / period)
    }

    fn fader_tempo(&self) -> f64 {
        (1.0 + self.tempo_fader as f64 * self.tempo_range.fraction()).max(0.0)
    }

    /// Tempo factor from fader or sync (without bends).
    pub fn tempo(&self) -> f64 {
        self.sync_tempo.unwrap_or_else(|| self.fader_tempo())
    }

    /// Effective BPM (track BPM × tempo), if the track has a grid.
    pub fn bpm(&self) -> Option<f64> {
        self.track_bpm().map(|b| b * self.tempo())
    }

    pub fn is_loaded(&self) -> bool {
        self.track.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn sync_enabled(&self) -> bool {
        self.sync
    }

    pub fn current_beat(&self) -> Option<f64> {
        self.beat_at(self.pos)
    }

    pub fn track(&self) -> Option<&Arc<TrackData>> {
        self.track.as_ref()
    }

    fn snap(&self, frames: f64, quantize: bool) -> f64 {
        if quantize
            && self.has_grid()
            && let Some(b) = self.beat_at(frames)
            && let Some(f) = self.frames_of_beat(b.round())
        {
            return f.max(0.0);
        }
        frames
    }

    fn beat_len_frames(&self) -> f64 {
        let bpm = self.track_bpm().unwrap_or(120.0);
        60.0 / bpm * self.sr
    }

    /// Any positional discontinuity: the keylock stretcher must re-prime.
    fn jump_to(&mut self, frames: f64) {
        self.pos = frames.clamp(0.0, self.frames().max(0.0));
        self.kl_active = false;
        self.kl_priming = false;
        self.at_end = false;
        if self.sync {
            self.sync_snap_pending = true;
        }
    }

    fn slip_action_active(&self) -> bool {
        self.loop_active
            || self.roll
            || self.reverse
            || (self.jog_touched && self.jog_mode == JogMode::Vinyl)
    }

    /// Call before and after any state change that may start or end a slip
    /// action.
    fn update_slip(&mut self, was_active: bool) {
        let now_active = self.slip_action_active();
        let slipping = self.slip || self.roll || self.roll_saved.is_some() || self.censor;
        if !was_active && now_active && slipping {
            self.slip_pos = self.pos;
            self.slip_diverged = true;
        } else if was_active && !now_active && self.slip_diverged {
            self.slip_diverged = false;
            let target = self.slip_pos;
            self.jump_to(target);
        }
    }

    fn hot_cue_frames(&self, hc: &HotCue) -> (f64, Option<f64>) {
        (hc.start * self.sr, hc.loop_end.map(|e| e * self.sr))
    }

    fn emit_hot_cue_set(&self, slot: usize, ctx: &mut RtCtx) {
        if let (Some(t), Some(cue)) = (&self.track, self.hot_cues[self.hot_bank][slot]) {
            ctx.event(Event::HotCueSet {
                deck: self.index,
                track: t.id,
                bank: self.hot_bank,
                slot,
                cue,
            });
        }
    }

    fn store_hot_cue(&mut self, slot: usize, ctx: &mut RtCtx) {
        let cue = match (self.loop_active, self.loop_in, self.loop_out) {
            (true, Some(i), Some(o)) => HotCue {
                start: i / self.sr,
                loop_end: Some(o / self.sr),
            },
            _ => HotCue {
                start: self.snap(self.pos, ctx.quantize) / self.sr,
                loop_end: None,
            },
        };
        self.hot_cues[self.hot_bank][slot] = Some(cue);
        self.emit_hot_cue_set(slot, ctx);
    }

    /// Jumps to a cue (optionally with loop). While playing with quantize the
    /// jump waits for the next beat so the new position starts on the beat.
    fn jump_to_cue(&mut self, start: f64, loop_range: Option<(f64, f64)>, ctx: &RtCtx) {
        if self.playing
            && ctx.quantize
            && self.has_grid()
            && !self.reverse
            && let Some(b) = self.beat_at(self.pos)
        {
            let next = if (b - b.round()).abs() < 1e-6 {
                b.round()
            } else {
                b.floor() + 1.0
            };
            if let Some(nf) = self.frames_of_beat(next) {
                let speed = self.tempo().max(1e-3);
                let in_frames = ((nf - self.pos) / speed).round().max(0.0) as usize;
                self.pending_jump = Some(PendingJump {
                    in_frames,
                    target: start,
                    loop_range,
                });
                return;
            }
        }
        self.perform_jump(start, loop_range);
    }

    fn perform_jump(&mut self, start: f64, loop_range: Option<(f64, f64)>) {
        let was = self.slip_action_active();
        if let Some((i, o)) = loop_range {
            self.loop_in = Some(i);
            self.loop_out = Some(o);
            self.loop_active = true;
            self.loop_beats = None;
        }
        self.jump_to(start);
        self.update_slip(was);
    }

    fn set_loop(&mut self, start: f64, end: f64, beats: Option<f64>) {
        let was = self.slip_action_active();
        self.loop_in = Some(start);
        self.loop_out = Some(end);
        self.loop_beats = beats;
        self.loop_active = true;
        if self.pos >= end {
            let len = end - start;
            let p = start + (self.pos - start).rem_euclid(len);
            self.jump_to(p);
        }
        // Inside the loop range the stretcher's look-ahead may already be
        // past the loop end: re-prime keylock.
        self.kl_active = false;
        self.update_slip(was);
    }

    fn exit_loop(&mut self) {
        let was = self.slip_action_active();
        self.loop_active = false;
        self.update_slip(was);
    }

    /// Start/end frames of an auto loop of `beats` beats at the playhead.
    fn auto_loop_range(&self, beats: f64, quantize: bool) -> (f64, f64) {
        let beats = beats.clamp(1.0 / 32.0, 512.0);
        if self.has_grid()
            && let Some(cur) = self.beat_at(self.pos)
        {
            let start_beat = if quantize {
                let unit = beats.min(1.0);
                (cur / unit + 1e-9).floor() * unit
            } else {
                cur
            };
            if let (Some(s), Some(e)) = (
                self.frames_of_beat(start_beat),
                self.frames_of_beat(start_beat + beats),
            ) {
                return (s, e);
            }
        }
        let len = beats * self.beat_len_frames();
        (self.pos, self.pos + len)
    }

    // ----- commands ------------------------------------------------------

    pub fn apply(&mut self, cmd: DeckCommand, ctx: &mut RtCtx) {
        if !matches!(cmd, DeckCommand::Load(_)) && self.track.is_none() {
            match cmd {
                DeckCommand::SetTempoFader(v) => self.tempo_fader = v.clamp(-1.0, 1.0),
                DeckCommand::SetTempoRange(r) => self.tempo_range = r,
                DeckCommand::SetKeylock(on) => self.keylock = on,
                DeckCommand::SetKeyShift(s) => {
                    self.key_shift = s.clamp(-12.0, 12.0);
                    self.key_sync = false;
                }
                DeckCommand::SetKeySync(on) => self.key_sync = on,
                DeckCommand::SetAutoCue(on) => self.auto_cue = on,
                DeckCommand::SetBeatJumpSize(b) => self.beat_jump_size = b.clamp(1.0 / 32.0, 512.0),
                DeckCommand::SetHotCueBank(b) if b < HOT_CUE_BANKS => self.hot_bank = b,
                DeckCommand::SetSync(on) => self.sync = on,
                DeckCommand::SetJogMode(m) => self.jog_mode = m,
                DeckCommand::SetSlip(on) => self.slip = on,
                DeckCommand::SetStemLevel { stem, level } => self.set_stem_level(stem, level),
                // Never drop track data on the audio thread.
                DeckCommand::SwapTrackData(t) => ctx.dispose_track(t),
                _ => {}
            }
            return;
        }
        match cmd {
            DeckCommand::Load(track) => self.load(track, ctx),
            DeckCommand::SwapTrackData(track) => self.swap_track_data(track, ctx),
            DeckCommand::SetStemLevel { stem, level } => self.set_stem_level(stem, level),
            DeckCommand::Unload => {
                if let Some(old) = self.track.take() {
                    ctx.dispose_track(old);
                }
                self.reset_transport();
                ctx.event(Event::TrackUnloaded { deck: self.index });
            }
            DeckCommand::Play => {
                if self.cue_preview || self.hot_preview.is_some() {
                    self.play_latched = true;
                }
                if !self.playing {
                    self.start_playing();
                }
            }
            DeckCommand::Pause => {
                self.playing = false;
                self.cue_preview = false;
                self.hot_preview = None;
                self.pending_jump = None;
            }
            DeckCommand::TogglePlay => {
                if self.playing && !self.cue_preview && self.hot_preview.is_none() {
                    self.playing = false;
                    self.pending_jump = None;
                } else {
                    self.apply(DeckCommand::Play, ctx);
                }
            }
            DeckCommand::Cue(pressed) => self.cue_button(pressed, ctx),
            DeckCommand::SetTempoFader(v) => {
                self.tempo_fader = v.clamp(-1.0, 1.0);
            }
            DeckCommand::SetTempoRange(r) => self.tempo_range = r,
            DeckCommand::SetKeylock(on) => {
                if on != self.keylock {
                    self.keylock = on;
                    self.kl_active = false;
                }
            }
            DeckCommand::SetKeyShift(s) => {
                self.key_shift = s.clamp(-12.0, 12.0);
                self.key_sync = false;
            }
            DeckCommand::SetKeySync(on) => self.key_sync = on,
            DeckCommand::SetTrackKey(k) => self.track_key = k,
            DeckCommand::SetAutoCue(on) => self.auto_cue = on,
            DeckCommand::SetBeatJumpSize(b) => self.beat_jump_size = b.clamp(1.0 / 32.0, 512.0),
            DeckCommand::BeatJumpStep { forward } => {
                let b = if forward { self.beat_jump_size } else { -self.beat_jump_size };
                self.beat_jump(b);
            }
            DeckCommand::SetHotCueBank(b) => {
                if b < HOT_CUE_BANKS {
                    self.hot_bank = b;
                }
            }
            DeckCommand::Censor(on) => self.set_censor(on),
            DeckCommand::SavedLoop(dir) => self.memory_cue(dir, true, ctx),
            DeckCommand::SetActiveLoop(range) => {
                self.active_loop = range
                    .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
                    .map(|(a, b)| (a * self.sr, b * self.sr));
            }
            DeckCommand::Slicer(c) => self.slicer_command(c, ctx),
            DeckCommand::Keyboard {
                slot,
                semitones,
                pressed,
            } if slot < HOT_CUES => {
                if pressed {
                    self.key_sync = false;
                    self.key_shift = semitones.clamp(-12.0, 12.0);
                }
                self.hot_cue_pad(slot, pressed, false, ctx);
            }
            DeckCommand::Keyboard { .. } => {}
            DeckCommand::SetSync(on) => {
                if on && !self.sync {
                    self.sync_snap_pending = true;
                }
                if !on && let Some(t) = self.sync_tempo.take() {
                    // Keep the synced tempo: move the (virtual) fader there.
                    let r = self.tempo_range.fraction();
                    self.tempo_fader = (((t - 1.0) / r) as f32).clamp(-1.0, 1.0);
                }
                self.sync = on;
            }
            DeckCommand::SetPitchBend(b) => self.bend = (b as f64).clamp(-0.9, 0.9),
            DeckCommand::SetJogMode(m) => self.jog_mode = m,
            DeckCommand::JogTouch(t) => {
                let was = self.slip_action_active();
                self.jog_touched = t;
                if t && self.jog_mode == JogMode::Vinyl {
                    self.jog_pending = 0.0;
                    self.scratch_speed = 0.0;
                    self.kl_active = false;
                }
                self.update_slip(was);
            }
            DeckCommand::JogTurn(rev) => {
                let rev = rev as f64;
                let scratch = self.jog_touched && self.jog_mode == JogMode::Vinyl;
                if scratch {
                    self.jog_pending += rev * SECONDS_PER_REVOLUTION * self.sr;
                } else if !self.playing {
                    // Search while paused.
                    let target = self.pos + rev * SECONDS_PER_REVOLUTION * self.sr;
                    self.jump_to(target);
                } else {
                    self.jog_bend_accum += rev;
                }
            }
            DeckCommand::HotCue { slot, pressed } if slot < HOT_CUES => {
                self.hot_cue_pad(slot, pressed, true, ctx)
            }
            DeckCommand::HotCue { .. } => {}
            DeckCommand::SetHotCue { slot } if slot < HOT_CUES => self.store_hot_cue(slot, ctx),
            DeckCommand::SetHotCueAt {
                slot,
                start,
                loop_end,
            } if slot < HOT_CUES => {
                self.hot_cues[self.hot_bank][slot] = Some(HotCue { start, loop_end });
                self.emit_hot_cue_set(slot, ctx);
            }
            DeckCommand::DeleteHotCue { slot } if slot < HOT_CUES => {
                if self.hot_cues[self.hot_bank][slot].take().is_some()
                    && let Some(t) = &self.track
                {
                    ctx.event(Event::HotCueDeleted {
                        deck: self.index,
                        track: t.id,
                        bank: self.hot_bank,
                        slot,
                    });
                }
            }
            DeckCommand::SetHotCue { .. }
            | DeckCommand::SetHotCueAt { .. }
            | DeckCommand::DeleteHotCue { .. } => {}
            DeckCommand::MemoryCue(dir) => self.memory_cue(dir, false, ctx),
            DeckCommand::LoopIn => {
                self.loop_in = Some(self.snap(self.pos, ctx.quantize));
                self.loop_out = None;
                self.loop_beats = None;
                self.exit_loop();
            }
            DeckCommand::LoopOut => {
                if let Some(i) = self.loop_in {
                    let mut o = self.snap(self.pos, ctx.quantize);
                    if o <= i + 1.0 {
                        o = self.pos;
                    }
                    if o > i + 1.0 {
                        self.set_loop(i, o, None);
                    }
                }
            }
            DeckCommand::LoopExit => self.exit_loop(),
            DeckCommand::Reloop => {
                if self.loop_active {
                    self.exit_loop();
                } else if let (Some(i), Some(o)) = (self.loop_in, self.loop_out) {
                    self.perform_jump(i, Some((i, o)));
                }
            }
            DeckCommand::AutoLoop { beats } => {
                if self.loop_active && self.loop_beats == Some(beats) && !self.roll {
                    self.exit_loop();
                } else {
                    let (s, e) = self.auto_loop_range(beats, ctx.quantize);
                    self.set_loop(s, e, Some(beats.clamp(1.0 / 32.0, 512.0)));
                }
            }
            DeckCommand::LoopHalve => self.resize_loop(0.5),
            DeckCommand::LoopDouble => self.resize_loop(2.0),
            DeckCommand::LoopRoll { beats, on } => self.loop_roll(beats, on, ctx),
            DeckCommand::SetSlip(on) => {
                if !on && self.slip_diverged && !self.roll {
                    // Turning slip off keeps the current playhead.
                    self.slip_diverged = false;
                }
                self.slip = on;
            }
            DeckCommand::BeatJump { beats } => self.beat_jump(beats),
            DeckCommand::SetReverse(on) => {
                let was = self.slip_action_active();
                self.reverse = on;
                self.kl_active = false;
                self.update_slip(was);
            }
            DeckCommand::Seek { seconds } => {
                self.pending_jump = None;
                self.jump_to(seconds * self.sr);
            }
            DeckCommand::NeedleSearch { fraction } => {
                self.pending_jump = None;
                let f = fraction.clamp(0.0, 1.0) * self.frames();
                self.jump_to(f);
            }
        }
    }

    fn load(&mut self, track: Arc<TrackData>, ctx: &mut RtCtx) {
        if let Some(old) = self.track.take() {
            ctx.dispose_track(old);
        }
        let id = track.id;
        let start = if self.auto_cue {
            track
                .memory_cues
                .first()
                .map_or(track.first_sound, |c| c.start)
                * self.sr
        } else {
            0.0
        };
        self.track = Some(track);
        self.reset_transport();
        self.pos = start;
        self.cue_point = start;
        self.hot_cues = self.track.as_ref().unwrap().hot_cues;
        self.hot_bank = 0;
        self.track_key = None;
        // New audio: no ramp from the previous track's state.
        self.stem_play.level = self.stem_target;
        self.stem_feed.level = self.stem_target;
        ctx.event(Event::TrackLoaded {
            deck: self.index,
            track: id,
        });
    }

    fn swap_track_data(&mut self, track: Arc<TrackData>, ctx: &mut RtCtx) {
        let same = self.track.as_ref().is_some_and(|t| t.id == track.id);
        if !same {
            ctx.dispose_track(track);
            return;
        }
        let had_stems = self.track.as_ref().is_some_and(|t| t.stems.is_some());
        if let Some(old) = self.track.replace(track) {
            ctx.dispose_track(old);
        }
        let end = self.frames();
        if self.pos > end {
            self.pos = end;
        }
        // The mix is unchanged, so the keylock stretcher keeps running; the
        // stem mix (if levels ≠ 1) fades in from unity instead of jumping.
        let has_stems = self.track.as_ref().is_some_and(|t| t.stems.is_some());
        if has_stems && !had_stems {
            self.stem_play.level = [1.0; 4];
            self.stem_feed.level = [1.0; 4];
        }
    }

    fn set_stem_level(&mut self, stem: crate::stems::Stem, level: f32) {
        let level = if level.is_finite() { level.clamp(0.0, 1.0) } else { 1.0 };
        self.stem_target[stem.index()] = level;
    }

    /// Whether frames must go through the stem mix (else: plain mix). Snaps
    /// the ramp when the track has no stems.
    fn stems_active(track: &TrackData, ramp: &mut StemRamp, target: &[f32; 4]) -> bool {
        if track.stems.is_none() {
            ramp.level = *target;
            return false;
        }
        !(ramp.settled(target) && *target == [1.0; 4])
    }

    fn reset_transport(&mut self) {
        self.pos = 0.0;
        self.playing = false;
        self.at_end = false;
        self.cue_point = 0.0;
        self.cue_preview = false;
        self.play_latched = false;
        self.hot_preview = None;
        self.loop_in = None;
        self.loop_out = None;
        self.loop_active = false;
        self.loop_beats = None;
        self.roll = false;
        self.roll_saved = None;
        self.slip_diverged = false;
        self.reverse = false;
        self.censor = false;
        self.active_loop = None;
        self.slicer = None;
        self.jog_pending = 0.0;
        self.scratch_speed = 0.0;
        self.pending_jump = None;
        self.kl_active = false;
        self.sync_tempo = None;
        if self.track.is_none() {
            self.hot_cues = [[None; HOT_CUES]; HOT_CUE_BANKS];
            self.track_key = None;
        }
    }

    fn start_playing(&mut self) {
        if self.at_end {
            return;
        }
        self.playing = true;
        if self.sync {
            self.sync_snap_pending = true;
        }
    }

    fn at_cue(&self) -> bool {
        (self.pos - self.cue_point).abs() < 1.0
    }

    fn cue_button(&mut self, pressed: bool, ctx: &mut RtCtx) {
        if pressed {
            if self.playing && !self.cue_preview {
                // Back cue.
                self.playing = false;
                self.hot_preview = None;
                self.pending_jump = None;
                let c = self.cue_point;
                self.jump_to(c);
            } else if !self.playing {
                if self.at_cue() {
                    self.cue_preview = true;
                    self.play_latched = false;
                    self.start_playing();
                } else {
                    let c = self.snap(self.pos, ctx.quantize);
                    self.cue_point = c;
                    self.jump_to(c);
                    if let Some(t) = &self.track {
                        ctx.event(Event::CuePointSet {
                            deck: self.index,
                            track: t.id,
                            position: c / self.sr,
                        });
                    }
                }
            }
        } else if self.cue_preview {
            self.cue_preview = false;
            if !self.play_latched {
                self.playing = false;
                let c = self.cue_point;
                self.jump_to(c);
            }
            self.play_latched = false;
        }
    }

    fn hot_cue_pad(&mut self, slot: usize, pressed: bool, quantized: bool, ctx: &mut RtCtx) {
        if pressed {
            let Some(hc) = self.hot_cues[self.hot_bank][slot] else {
                if quantized {
                    self.store_hot_cue(slot, ctx);
                }
                return;
            };
            let (start, end) = self.hot_cue_frames(&hc);
            let range = end.map(|e| (start, e));
            if self.playing && self.hot_preview.is_none() && !self.cue_preview {
                if quantized {
                    self.jump_to_cue(start, range, ctx);
                } else {
                    self.pending_jump = None;
                    self.perform_jump(start, range);
                }
            } else {
                // Paused (or previewing): jump and preview while held.
                self.pending_jump = None;
                self.perform_jump(start, range);
                self.hot_preview = Some(slot);
                self.cue_preview = false;
                self.play_latched = false;
                self.start_playing();
            }
        } else if self.hot_preview == Some(slot) {
            self.hot_preview = None;
            if !self.play_latched {
                self.playing = false;
                if let Some(hc) = self.hot_cues[self.hot_bank][slot] {
                    let (s, _) = self.hot_cue_frames(&hc);
                    self.jump_to(s);
                }
            }
            self.play_latched = false;
        }
    }

    /// Memory cue (or, with `loops_only`, saved loop) navigation.
    fn memory_cue(&mut self, dir: MemoryCueJump, loops_only: bool, ctx: &mut RtCtx) {
        let Some(track) = &self.track else { return };
        let t = self.pos / self.sr;
        let cues = track
            .memory_cues
            .iter()
            .filter(|c| !loops_only || c.loop_end.is_some());
        let found = match dir {
            MemoryCueJump::Next => cues.clone().find(|c| c.start > t + 0.01).copied(),
            MemoryCueJump::Previous => cues.clone().rev().find(|c| c.start < t - 0.01).copied(),
            MemoryCueJump::Index(i) => cues.clone().nth(i).copied(),
        };
        if let Some(c) = found {
            let id = track.id;
            let start = c.start * self.sr;
            self.cue_point = start;
            ctx.event(Event::CuePointSet {
                deck: self.index,
                track: id,
                position: c.start,
            });
            let range = c.loop_end.map(|e| (start, e * self.sr));
            if self.playing {
                self.jump_to_cue(start, range, ctx);
            } else {
                self.perform_jump(start, range);
            }
        }
    }

    fn resize_loop(&mut self, factor: f64) {
        let (Some(i), Some(o)) = (self.loop_in, self.loop_out) else {
            return;
        };
        let len = (o - i) * factor;
        if len < 8.0 || len > self.sr * 1200.0 {
            return;
        }
        let beats = self
            .loop_beats
            .map(|b| (b * factor).clamp(1.0 / 32.0, 512.0));
        let new_out = match (beats, self.beat_at(i)) {
            (Some(_), Some(bi)) if self.has_grid() => {
                let b = self.loop_beats.unwrap() * factor;
                self.frames_of_beat(bi + b).unwrap_or(i + len)
            }
            _ => i + len,
        };
        self.loop_out = Some(new_out);
        self.loop_beats = beats;
        if self.loop_active && self.pos >= new_out {
            let l = new_out - i;
            let p = i + (self.pos - i).rem_euclid(l);
            self.jump_to(p);
        }
    }

    fn loop_roll(&mut self, beats: f64, on: bool, ctx: &mut RtCtx) {
        if on {
            if self.roll_saved.is_none() {
                self.roll_saved = Some((
                    self.loop_in,
                    self.loop_out,
                    self.loop_active,
                    self.loop_beats,
                ));
            }
            if !self.slip_diverged {
                self.slip_pos = self.pos;
                self.slip_diverged = true;
            }
            self.roll = true;
            let (s, e) = self.auto_loop_range(beats, ctx.quantize);
            self.loop_in = Some(s);
            self.loop_out = Some(e);
            self.loop_beats = Some(beats);
            self.loop_active = true;
            self.kl_active = false;
        } else if self.roll {
            self.roll = false;
            if let Some((i, o, a, b)) = self.roll_saved.take() {
                self.loop_in = i;
                self.loop_out = o;
                self.loop_active = a;
                self.loop_beats = b;
            }
            if self.slip_diverged && !(self.slip && self.slip_action_active()) {
                self.slip_diverged = false;
                let t = self.slip_pos;
                self.jump_to(t);
            }
        }
    }

    fn set_censor(&mut self, on: bool) {
        if on == self.censor {
            return;
        }
        let was = self.slip_action_active();
        if on {
            self.censor = true;
            self.reverse = true;
            self.kl_active = false;
            self.update_slip(was);
        } else {
            self.reverse = false;
            self.kl_active = false;
            self.update_slip(was);
            self.censor = false;
        }
    }

    /// Slicer domain containing `frames`, for `beats` beats.
    fn slicer_domain(&self, frames: f64, beats: f64) -> (Option<f64>, f64, f64) {
        if self.has_grid()
            && let Some(b) = self.beat_at(frames)
        {
            let sb = (b / beats + 1e-9).floor() * beats;
            if let (Some(s), Some(e)) = (self.frames_of_beat(sb), self.frames_of_beat(sb + beats)) {
                return (Some(sb), s, e);
            }
        }
        let len = beats * self.beat_len_frames();
        (None, frames, frames + len)
    }

    /// Where normal playback is (slip shadow while diverged).
    fn reference_pos(&self) -> f64 {
        if self.slip_diverged { self.slip_pos } else { self.pos }
    }

    fn slicer_command(&mut self, cmd: SlicerCommand, ctx: &mut RtCtx) {
        match cmd {
            SlicerCommand::Enable { beats, loop_mode } => {
                if let Some(sl) = self.slicer
                    && sl.held.is_some()
                {
                    self.slicer_release();
                }
                let beats = if beats.is_finite() { beats.clamp(1.0, 64.0) } else { 8.0 };
                let (start_beat, start, end) = self.slicer_domain(self.reference_pos(), beats);
                self.slicer = Some(Slicer {
                    beats,
                    loop_mode,
                    start_beat,
                    start,
                    end,
                    held: None,
                });
                if loop_mode {
                    self.set_loop(start, end, Some(beats));
                }
            }
            SlicerCommand::Disable => {
                if let Some(sl) = self.slicer {
                    if sl.held.is_some() {
                        self.slicer_release();
                    }
                    if sl.loop_mode
                        && self.loop_active
                        && self.loop_in == Some(sl.start)
                        && self.loop_out == Some(sl.end)
                    {
                        self.exit_loop();
                    }
                }
                self.slicer = None;
            }
            SlicerCommand::Pad { slice, pressed } if slice < SLICER_SLICES => {
                let Some(sl) = self.slicer else { return };
                if pressed {
                    self.slicer_press(sl, slice, ctx.quantize);
                } else if sl.held == Some(slice) {
                    self.slicer_release();
                }
            }
            SlicerCommand::Pad { .. } => {}
        }
    }

    fn slicer_press(&mut self, mut sl: Slicer, slice: usize, quantize: bool) {
        let n = SLICER_SLICES as f64;
        let slice_beats = sl.beats / n;
        let (s0, s1, target) = match sl.start_beat {
            Some(sb) => {
                let b0 = sb + slice as f64 * slice_beats;
                let (Some(s0), Some(s1)) = (self.frames_of_beat(b0), self.frames_of_beat(b0 + slice_beats)) else {
                    return;
                };
                let target = match self.beat_at(self.pos) {
                    Some(cur) if quantize => {
                        let phase = (cur - sb).rem_euclid(slice_beats);
                        self.frames_of_beat(b0 + phase).unwrap_or(s0)
                    }
                    _ => s0,
                };
                (s0, s1, target)
            }
            None => {
                let len = (sl.end - sl.start) / n;
                let s0 = sl.start + slice as f64 * len;
                let target = if quantize {
                    s0 + (self.pos - sl.start).rem_euclid(len)
                } else {
                    s0
                };
                (s0, s0 + len, target)
            }
        };
        if self.roll_saved.is_none() {
            self.roll_saved = Some((self.loop_in, self.loop_out, self.loop_active, self.loop_beats));
        }
        if !self.slip_diverged {
            self.slip_pos = self.pos;
            self.slip_diverged = true;
        }
        self.roll = true;
        self.pending_jump = None;
        self.loop_in = Some(s0);
        self.loop_out = Some(s1);
        self.loop_beats = Some(slice_beats);
        self.loop_active = true;
        self.jump_to(target.clamp(s0, s1 - 1.0));
        sl.held = Some(slice);
        self.slicer = Some(sl);
    }

    fn slicer_release(&mut self) {
        if let Some(sl) = &mut self.slicer {
            sl.held = None;
        }
        if self.roll {
            self.roll = false;
            if let Some((i, o, a, b)) = self.roll_saved.take() {
                self.loop_in = i;
                self.loop_out = o;
                self.loop_active = a;
                self.loop_beats = b;
            }
            if self.slip_diverged && !(self.slip && self.slip_action_active()) {
                self.slip_diverged = false;
                let t = self.slip_pos;
                self.jump_to(t);
            }
        }
    }

    /// Moves a non-looping slicer domain along with playback.
    fn update_slicer(&mut self) {
        let Some(mut sl) = self.slicer else { return };
        let r = self.reference_pos();
        if sl.loop_mode {
            // The slip shadow loops inside the domain like the playhead.
            let len = sl.end - sl.start;
            if self.slip_diverged && len > 0.0 && self.slip_pos >= sl.end {
                self.slip_pos = sl.start + (self.slip_pos - sl.start).rem_euclid(len);
            }
            return;
        }
        if r >= sl.end || r < sl.start {
            let (sb, s, e) = self.slicer_domain(r, sl.beats);
            sl.start_beat = sb;
            sl.start = s;
            sl.end = e;
            self.slicer = Some(sl);
        }
    }

    fn slicer_state(&self) -> SlicerState {
        let Some(sl) = self.slicer else {
            return SlicerState::default();
        };
        let len = sl.end - sl.start;
        let r = self.reference_pos();
        let current = if len > 0.0 {
            (((r - sl.start) / len * SLICER_SLICES as f64).floor()).clamp(0.0, SLICER_SLICES as f64 - 1.0) as u8
        } else {
            0
        };
        SlicerState {
            active: true,
            loop_mode: sl.loop_mode,
            beats: sl.beats,
            start: sl.start / self.sr,
            end: sl.end / self.sr,
            current,
            held: sl.held.map(|h| h as u8),
        }
    }

    /// Audible pitch offset in semitones (key shift + tempo pitch without
    /// keylock).
    fn pitch_semitones(&self) -> f64 {
        let tempo = if self.keylock {
            0.0
        } else {
            12.0 * self.tempo().max(1e-6).log2()
        };
        self.key_shift as f64 + tempo
    }

    /// Sounding key (major-equivalent pitch class, fractional semitones).
    pub fn key_pitch(&self) -> Option<f64> {
        self.track_key.map(|k| major_tonic(k) + self.pitch_semitones())
    }

    fn beat_jump(&mut self, beats: f64) {
        let target = match self.beat_at(self.pos) {
            Some(b) if self.has_grid() => self.frames_of_beat(b + beats),
            _ => None,
        }
        .unwrap_or(self.pos + beats * self.beat_len_frames());
        let shift = target - self.pos;
        if self.loop_active
            && let (Some(i), Some(o)) = (self.loop_in, self.loop_out)
        {
            self.loop_in = Some(i + shift);
            self.loop_out = Some(o + shift);
        }
        if self.slip_diverged {
            self.slip_pos += shift;
        }
        self.jump_to(target);
    }

    // ----- per-block processing -----------------------------------------

    /// Updates sync tempo/phase correction and computes the block's speed.
    pub fn prepare(&mut self, frames: usize, sync: Option<SyncTarget>, is_master: bool) {
        let block_s = frames as f64 / self.sr;
        // Jog bend in bend mode / outer ring: proportional to turn speed.
        let v = self.jog_bend_accum / block_s.max(1e-6); // rev/s
        self.jog_bend_accum = 0.0;
        let target_bend = (v * 0.25).clamp(-0.9, 0.9);
        let a = (-block_s / 0.05).exp();
        self.jog_bend = self.jog_bend * a + target_bend * (1.0 - a);
        if self.jog_bend.abs() < 1e-5 {
            self.jog_bend = 0.0;
        }

        if self.key_sync
            && !is_master
            && let (Some(t), Some(k)) = (sync.and_then(|t| t.key), self.track_key)
        {
            let tempo = if self.keylock {
                0.0
            } else {
                12.0 * self.tempo().max(1e-6).log2()
            };
            let d = (t - major_tonic(k) - tempo + 6.0).rem_euclid(12.0) - 6.0;
            self.key_shift = d as f32;
        }

        self.phase_correction = 0.0;
        if self.sync && !is_master && self.track.is_some() {
            if let (Some(t), Some(my_bpm)) = (sync, self.track_bpm()) {
                // Choose half/double tempo multiples so the ratio stays near 1.
                let mut k = 1.0;
                let mut ratio = t.bpm / my_bpm;
                if ratio > 1.5 {
                    k = 0.5;
                    ratio *= 0.5;
                } else if ratio < 0.75 {
                    k = 2.0;
                    ratio *= 2.0;
                }
                self.sync_tempo = Some(ratio);
                let bending = self.bend != 0.0 || self.jog_bend != 0.0 || self.jog_touched;
                if t.playing
                    && self.playing
                    && !bending
                    && !self.reverse
                    && let Some(my_beat) = self.beat_at(self.pos)
                {
                    let target_phase = (t.beat * k).rem_euclid(1.0);
                    let my_phase = my_beat.rem_euclid(1.0);
                    let mut err = target_phase - my_phase;
                    if err >= 0.5 {
                        err -= 1.0;
                    } else if err < -0.5 {
                        err += 1.0;
                    }
                    let period = self.beat_len_frames();
                    let err_frames = err * period;
                    if self.sync_snap_pending {
                        let target = self.pos + err_frames;
                        self.jump_to(target.max(0.0));
                        self.sync_snap_pending = false;
                    } else if err_frames.abs() > 0.5 {
                        // Remove the error over ~50 ms.
                        let c = err_frames / (self.sr * 0.05) / ratio;
                        self.phase_correction =
                            c.clamp(-MAX_PHASE_CORRECTION, MAX_PHASE_CORRECTION);
                    }
                }
            }
        } else if let Some(t) = self.sync_tempo.take() {
            // Became master (or lost the master): keep the synced tempo by
            // moving the virtual fader there.
            let r = self.tempo_range.fraction();
            self.tempo_fader = (((t - 1.0) / r) as f32).clamp(-1.0, 1.0);
        }

        let scratching = self.jog_touched && self.jog_mode == JogMode::Vinyl;
        if scratching || (self.scratch_speed != 0.0 && !self.playing) {
            // Follow the jog: consume pending movement with a short time constant.
            let alpha = 1.0 - (-block_s / SCRATCH_SMOOTHING_S).exp();
            let mv = self.jog_pending * alpha;
            self.jog_pending -= mv;
            self.scratch_speed = mv / frames.max(1) as f64;
            if !scratching && self.jog_pending.abs() < 1.0 {
                self.scratch_speed = 0.0;
                self.jog_pending = 0.0;
            }
            self.speed = self.scratch_speed;
        } else if self.playing {
            self.scratch_speed = 0.0;
            let s = self.tempo() * (1.0 + self.bend + self.jog_bend + self.phase_correction);
            self.speed = if self.reverse { -s } else { s };
        } else {
            self.speed = 0.0;
        }
    }

    fn use_keylock(&self) -> bool {
        (self.keylock || self.key_shift != 0.0)
            && self.stretcher.is_some()
            && self.playing
            && self.speed > 0.05
            && !(self.jog_touched && self.jog_mode == JogMode::Vinyl)
            && !self.reverse
    }

    /// Stretcher pitch scale: key shift, plus the speed without keylock.
    fn pitch_scale(&self) -> f64 {
        let shift = 2f64.powf(self.key_shift as f64 / 12.0);
        if self.keylock { shift } else { shift * self.speed }
    }

    /// Renders `frames` frames of interleaved stereo into `out`.
    pub fn render(&mut self, out: &mut [f32], frames: usize, ctx: &mut RtCtx) {
        let out = &mut out[..frames * 2];
        if self.track.is_none() {
            out.fill(0.0);
            return;
        }
        let mut done = 0;
        while done < frames {
            let mut seg = frames - done;
            if let Some(pj) = &self.pending_jump {
                seg = seg.min(pj.in_frames);
            }
            // Armed active loop: stop the segment where the playhead reaches it.
            let arm = self
                .active_loop
                .filter(|&(s, _)| !self.loop_active && self.speed > 0.0 && self.pos < s);
            if let Some((s, _)) = arm {
                let k = ((s - self.pos) / self.speed).ceil().max(1.0);
                if k < seg as f64 {
                    seg = k as usize;
                }
            }
            if seg > 0 {
                let before = self.pos;
                self.render_segment(&mut out[done * 2..(done + seg) * 2], seg, ctx);
                done += seg;
                if let Some((s, e)) = arm
                    && before < s
                    && self.pos >= s
                    && self.pos < e
                    && !self.loop_active
                {
                    self.set_loop(s, e, None);
                }
            }
            if let Some(pj) = &mut self.pending_jump {
                pj.in_frames -= seg.min(pj.in_frames);
                if pj.in_frames == 0 {
                    let pj = self.pending_jump.take().unwrap();
                    self.perform_jump(pj.target, pj.loop_range);
                }
            }
        }
        // Slip shadow advances at the plain tempo.
        if self.slip_diverged && self.playing {
            self.slip_pos = (self.slip_pos + self.tempo() * frames as f64).min(self.frames());
        }
        self.update_slicer();
    }

    fn render_segment(&mut self, out: &mut [f32], n: usize, ctx: &mut RtCtx) {
        if self.speed == 0.0 {
            out.fill(0.0);
            self.kl_active = false;
            self.kl_priming = false;
            return;
        }
        if self.use_keylock() {
            if self.kl_active {
                self.render_keylock(out, n);
            } else {
                self.render_priming(out, n);
            }
        } else {
            self.kl_active = false;
            self.kl_priming = false;
            self.render_resample(out, n);
        }
        self.check_end(ctx);
    }

    fn check_end(&mut self, ctx: &mut RtCtx) {
        let end = self.frames();
        let scratching = self.jog_touched && self.jog_mode == JogMode::Vinyl;
        if self.pos >= end && self.speed > 0.0 && !scratching {
            self.pos = end;
            if self.playing {
                self.playing = false;
                self.cue_preview = false;
                self.hot_preview = None;
                self.at_end = true;
                self.kl_active = false;
                ctx.event(Event::TrackEnded { deck: self.index });
            }
        }
        if self.pos < 0.0 {
            self.pos = 0.0;
        }
    }

    /// Frames the playhead travels (forward, following an active loop)
    /// until it reaches `target`, or `None` if it has already passed it.
    fn frames_until(&self, target: f64) -> Option<f64> {
        let d = target - self.pos;
        if d >= 0.0 {
            return Some(d);
        }
        match (self.loop_active, self.loop_in, self.loop_out) {
            (true, Some(i), Some(o)) if o > i && target >= i && self.pos <= o => Some((o - self.pos) + (target - i)),
            _ => None,
        }
    }

    /// Fills `kl_in[..req]` with input frames from `kl_feed_pos` (stem
    /// levels applied, following an active loop) and advances it.
    fn fill_feed(&mut self, track: &TrackData, req: usize, loop_range: Option<(i64, i64)>) {
        let frames = track.frames() as i64;
        let [l, r] = &mut self.kl_in;
        let mut p = self.kl_feed_pos;
        let stems = Self::stems_active(track, &mut self.stem_feed, &self.stem_target)
            .then_some(track.stems.as_ref())
            .flatten();
        for k in 0..req {
            if let Some(st) = stems {
                self.stem_feed.tick(&self.stem_target);
                let c = self.stem_feed.coefs();
                (l[k], r[k]) = frame_at(&track.samples, st, p as isize, &c);
            } else if p < 0 || p >= frames {
                l[k] = 0.0;
                r[k] = 0.0;
            } else {
                let j = p as usize * 2;
                l[k] = track.samples[j];
                r[k] = track.samples[j + 1];
            }
            p += 1;
            if let Some((i, o)) = loop_range
                && p >= o
            {
                p = i + (p - o);
            }
        }
        self.kl_feed_pos = p;
    }

    /// Feeds the stretcher from `kl_feed_pos` with at most `budget` input
    /// frames, until `need` output frames are available.
    fn kl_feed(&mut self, st: &mut Stretcher, track: &TrackData, need: usize, budget: usize) {
        let loop_range = match (self.loop_active, self.loop_in, self.loop_out) {
            (true, Some(i), Some(o)) if o - i >= 2.0 => Some((i.round() as i64, o.round() as i64)),
            _ => None,
        };
        let mut fed = 0;
        while st.available() < need && fed < budget {
            let req = st.samples_required().clamp(64, MAX_FEED).min(budget - fed).max(64);
            self.fill_feed(track, req, loop_range);
            let [l, r] = &self.kl_in;
            st.process(&l[..req], &r[..req]);
            fed += req;
        }
    }

    /// Drops the stretcher's start delay as its output becomes available.
    fn kl_drop_discard(&mut self, st: &mut Stretcher) {
        while self.kl_discard > 0 {
            let [l, r] = &mut self.kl_out;
            let k = self.kl_discard.min(l.len()).min(st.available());
            if k == 0 {
                break;
            }
            let got = st.retrieve(&mut l[..k], &mut r[..k]);
            if got == 0 {
                break;
            }
            self.kl_discard -= got;
        }
    }

    /// Keylock (re-)start without a load spike: the stretcher is primed a
    /// little at a time while the resampler plays; at `kl_target` the output
    /// crossfades into the stretcher.
    fn render_priming(&mut self, out: &mut [f32], n: usize) {
        let Some(mut st) = self.stretcher.take() else {
            self.render_resample(out, n);
            return;
        };
        let Some(track) = self.track.take() else {
            self.stretcher = Some(st);
            return;
        };
        // Input fed per block: twice real time keeps a prime well inside the
        // block budget even for the R3 engine and several decks at once.
        let budget = (n * 2).max(256);
        let restart = !self.kl_priming || self.frames_until(self.kl_target).is_none();
        if restart {
            st.reset();
            st.set_time_ratio(1.0 / self.speed);
            let pad = st.preferred_start_pad();
            let delay = st.start_delay();
            let prime = (pad + delay + n) as f64;
            // The playhead moves `speed` frames per output frame while the
            // stretcher is fed `budget / n` input frames per output frame.
            let ahead = prime * self.speed * n as f64 / budget as f64 * 1.5 + 2.0 * n as f64 * self.speed;
            let mut target = (self.pos + ahead).round();
            if let (true, Some(i), Some(o)) = (self.loop_active, self.loop_in, self.loop_out)
                && o > i
                && target >= o
            {
                target = i + (target - o).rem_euclid(o - i);
            }
            self.kl_target = target;
            self.kl_feed_pos = target as i64 - pad as i64;
            self.kl_discard = delay;
            self.kl_priming = true;
        }
        st.set_time_ratio(1.0 / self.speed);
        st.set_pitch_scale(self.pitch_scale());
        self.kl_feed(&mut st, &track, self.kl_discard + n, budget);
        self.kl_drop_discard(&mut st);
        let ready = self.kl_discard == 0 && st.available() >= n;
        let until = self.frames_until(self.kl_target).unwrap_or(f64::MAX);
        let k = ((until / self.speed).ceil() as usize).min(n);
        self.track = Some(track);
        self.stretcher = Some(st);
        // Resampler up to the target.
        self.render_resample(&mut out[..k * 2], k);
        if k >= n || !ready {
            if k < n {
                self.render_resample(&mut out[k * 2..], n - k);
            }
            return;
        }
        // Switch at the target: render both and crossfade over 64 frames.
        let m = n - k;
        let saved = self.pos;
        let mut mix = std::mem::take(&mut self.kl_mix);
        self.render_resample(&mut mix[..m * 2], m);
        self.pos = saved;
        self.kl_priming = false;
        self.kl_active = true;
        self.render_keylock(&mut out[k * 2..], m);
        let fade = m.min(64);
        for i in 0..fade {
            let w = (i + 1) as f32 / (fade + 1) as f32;
            for c in 0..2 {
                let j = (k + i) * 2 + c;
                out[j] = out[j] * w + mix[i * 2 + c] * (1.0 - w);
            }
        }
        self.kl_mix = mix;
    }

    fn render_resample(&mut self, out: &mut [f32], n: usize) {
        {
            let track = self.track.as_ref().unwrap();
            if Self::stems_active(track, &mut self.stem_play, &self.stem_target) {
                self.render_resample_stems(out, n);
                return;
            }
        }
        let track = self.track.as_ref().unwrap();
        let s = &track.samples;
        let total = (s.len() / 2) as isize;
        let speed = self.speed;
        let mut pos = self.pos;
        let loop_check = self.loop_active && self.loop_in.is_some() && self.loop_out.is_some();
        let (li, lo) = (self.loop_in.unwrap_or(0.0), self.loop_out.unwrap_or(0.0));
        let get = |i: isize| -> (f32, f32) {
            if i >= 0 && i < total {
                let k = i as usize * 2;
                (s[k], s[k + 1])
            } else {
                (0.0, 0.0)
            }
        };
        for f in out.as_chunks_mut::<2>().0.iter_mut().take(n) {
            let ip = pos.floor();
            let t = (pos - ip) as f32;
            let i = ip as isize;
            if i >= 1 && i + 2 < total {
                let k = i as usize * 2;
                let w = &s[k - 2..k + 6];
                f[0] = hermite(w[0], w[2], w[4], w[6], t);
                f[1] = hermite(w[1], w[3], w[5], w[7], t);
            } else {
                let (a0, a1) = get(i - 1);
                let (b0, b1) = get(i);
                let (c0, c1) = get(i + 1);
                let (d0, d1) = get(i + 2);
                f[0] = hermite(a0, b0, c0, d0, t);
                f[1] = hermite(a1, b1, c1, d1, t);
            }
            pos += speed;
            if loop_check && lo > li {
                let len = lo - li;
                if speed > 0.0 && pos >= lo {
                    pos -= len * ((pos - lo) / len).floor().max(0.0) + len;
                } else if speed < 0.0 && pos < li {
                    pos += len * ((li - pos) / len).floor().max(0.0) + len;
                }
            }
            if speed < 0.0 && pos < 0.0 {
                pos = 0.0;
            }
        }
        self.pos = pos;
    }

    /// [`Deck::render_resample`] with stem levels applied per source frame.
    fn render_resample_stems(&mut self, out: &mut [f32], n: usize) {
        let track = self.track.as_ref().unwrap();
        let Some(stems) = track.stems.as_ref() else {
            return;
        };
        let s = &track.samples;
        let speed = self.speed;
        let mut pos = self.pos;
        let loop_check = self.loop_active && self.loop_in.is_some() && self.loop_out.is_some();
        let (li, lo) = (self.loop_in.unwrap_or(0.0), self.loop_out.unwrap_or(0.0));
        let target = self.stem_target;
        let ramp = &mut self.stem_play;
        for f in out.as_chunks_mut::<2>().0.iter_mut().take(n) {
            ramp.tick(&target);
            let c = ramp.coefs();
            let ip = pos.floor();
            let t = (pos - ip) as f32;
            let i = ip as isize;
            let (a0, a1) = frame_at(s, stems, i - 1, &c);
            let (b0, b1) = frame_at(s, stems, i, &c);
            let (c0, c1) = frame_at(s, stems, i + 1, &c);
            let (d0, d1) = frame_at(s, stems, i + 2, &c);
            f[0] = hermite(a0, b0, c0, d0, t);
            f[1] = hermite(a1, b1, c1, d1, t);
            pos += speed;
            if loop_check && lo > li {
                let len = lo - li;
                if speed > 0.0 && pos >= lo {
                    pos -= len * ((pos - lo) / len).floor().max(0.0) + len;
                } else if speed < 0.0 && pos < li {
                    pos += len * ((li - pos) / len).floor().max(0.0) + len;
                }
            }
            if speed < 0.0 && pos < 0.0 {
                pos = 0.0;
            }
        }
        self.pos = pos;
    }

    fn render_keylock(&mut self, out: &mut [f32], n: usize) {
        // Temporarily move the Arc out (no refcount traffic, never dropped here).
        let Some(track) = self.track.take() else {
            return;
        };
        let Some(mut st) = self.stretcher.take() else {
            self.track = Some(track);
            return;
        };
        st.set_time_ratio(1.0 / self.speed);
        st.set_pitch_scale(self.pitch_scale());
        let loop_range = match (self.loop_active, self.loop_in, self.loop_out) {
            (true, Some(i), Some(o)) if o - i >= 2.0 => Some((i.round() as i64, o.round() as i64)),
            _ => None,
        };
        // Feed until enough output is available.
        let mut guard = 0;
        while st.available() < n + self.kl_discard && guard < 64 {
            guard += 1;
            let req = st.samples_required().clamp(64, MAX_FEED);
            self.fill_feed(&track, req, loop_range);
            let [l, r] = &self.kl_in;
            st.process(&l[..req], &r[..req]);
        }
        // Drop the start delay after a (re-)prime.
        while self.kl_discard > 0 {
            let [l, r] = &mut self.kl_out;
            let k = self.kl_discard.min(l.len()).min(st.available());
            if k == 0 {
                break;
            }
            let got = st.retrieve(&mut l[..k], &mut r[..k]);
            if got == 0 {
                break;
            }
            self.kl_discard -= got;
        }
        let [l, r] = &mut self.kl_out;
        let want = n.min(st.available());
        let got = if want > 0 { st.retrieve(&mut l[..want], &mut r[..want]) } else { 0 };
        for (i, f) in out.as_chunks_mut::<2>().0.iter_mut().take(n).enumerate() {
            if i < got {
                f[0] = l[i];
                f[1] = r[i];
            } else {
                f[0] = 0.0;
                f[1] = 0.0;
            }
        }
        self.stretcher = Some(st);
        self.track = Some(track);
        let mut p = self.pos + self.speed * n as f64;
        if let (true, Some(i), Some(o)) = (self.loop_active, self.loop_in, self.loop_out)
            && o > i
            && p >= o
        {
            p = i + (p - o).rem_euclid(o - i);
        }
        self.pos = p;
    }

    pub fn state(&self, is_master: bool) -> DeckState {
        let sr = self.sr;
        let track_bpm = self.track_bpm().unwrap_or(0.0);
        let beat = self.beat_at(self.pos).unwrap_or(0.0);
        let beat_in_bar = self
            .track
            .as_ref()
            .and_then(|t| {
                t.beatgrid
                    .beat_index_at(self.pos / sr)
                    .map(|i| t.beatgrid.beats[i].bar_position)
            })
            .unwrap_or(0);
        DeckState {
            track: self.track.as_ref().map(|t| t.id),
            duration: self.frames() / sr,
            position: self.pos / sr,
            playing: self.playing,
            speed: self.speed,
            tempo: self.tempo(),
            tempo_fader: self.tempo_fader,
            tempo_range: self.tempo_range,
            track_bpm,
            bpm: track_bpm * self.tempo(),
            beat,
            beat_in_bar,
            keylock: self.keylock,
            key_shift: self.key_shift,
            key_sync: self.key_sync,
            track_key: self.track_key,
            pitch_semitones: self.pitch_semitones() as f32,
            sync: self.sync,
            is_master,
            slip: self.slip,
            slip_position: self.slip_diverged.then(|| self.slip_pos / sr),
            reverse: self.reverse,
            censor: self.censor,
            scratching: self.jog_touched && self.jog_mode == JogMode::Vinyl,
            jog_mode: self.jog_mode,
            cue_point: self.cue_point / sr,
            auto_cue: self.auto_cue,
            hot_cues: self.hot_cues[self.hot_bank],
            hot_cue_bank: self.hot_bank,
            hot_cue_banks: self.hot_cues,
            loop_state: LoopState {
                active: self.loop_active,
                start: self.loop_in.map(|v| v / sr),
                end: self.loop_out.map(|v| v / sr),
                beats: self.loop_beats,
                roll: self.roll,
            },
            active_loop: self.active_loop.map(|(a, b)| (a / sr, b / sr)),
            beat_jump_size: self.beat_jump_size,
            slicer: self.slicer_state(),
            at_end: self.at_end,
            stems_available: self.track.as_ref().is_some_and(|t| t.stems.is_some()),
            stem_levels: self.stem_target,
        }
    }
}
