//! The control-side [`Engine`] handle and the real-time [`EngineProcessor`].

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::beatfx::{BeatClock, FxUnit};
use crate::command::{BeatFxCommand, Command, CrossfaderCurve, DeckCommand, FxTarget, MixerCommand};
use crate::config::{BEAT_FX_SLOTS, BEAT_FX_UNITS, EngineConfig, MAX_SENDERS};
use crate::deck::{Deck, SyncTarget};
use crate::dsp::limiter::Limiter;
use crate::dsp::meter::Meter;
use crate::dsp::smooth::Smoothed;
use crate::dsp::soft_clip;
use crate::config::MAX_DECKS;
use crate::mixer::{Channel, crossfader_gains, level_gain};
use crate::rt::{Garbage, RtCtx};
use crate::sampler::Sampler;
use crate::state::{AudibleState, EngineState, Event};
use crate::track::TrackData;

const COMMAND_CAPACITY: usize = 1024;
const EVENT_CAPACITY: usize = 1024;
const GARBAGE_CAPACITY: usize = 256;
/// Record ring length in seconds of stereo audio.
const RECORD_SECONDS: usize = 10;
/// Contribution RMS above which a playing deck counts as audible (−40 dBFS).
const AUDIBLE_THRESHOLD: f32 = 0.01;
/// Longest delay / roll / tape history of the two Beat FX units, seconds.
const BEAT_FX_SECONDS: f32 = 8.0;
/// Same for the per-channel Pad FX units.
const PAD_FX_SECONDS: f32 = 4.0;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("engine command queue is full")]
    QueueFull,
    #[error("too many command senders (max {MAX_SENDERS})")]
    TooManySenders,
}

/// Output buffers for one call of [`EngineProcessor::process`]. All are
/// interleaved stereo with at least `frames * 2` samples.
pub struct Outputs<'a> {
    pub master: &'a mut [f32],
    /// Headphone (cue) output; `None` if the device has no headphone pair.
    pub headphones: Option<&'a mut [f32]>,
    /// Booth output (master bus × booth level, independent of the master
    /// level); `None` if not routed.
    pub booth: Option<&'a mut [f32]>,
}

/// Non-real-time handle: send commands, read the state snapshot and events.
pub struct Engine {
    config: EngineConfig,
    commands: rtrb::Producer<Command>,
    state: triple_buffer::Output<EngineState>,
    events: rtrb::Consumer<Event>,
    garbage: rtrb::Consumer<Garbage>,
    record: Option<RecordTap>,
    active_senders: Arc<AtomicUsize>,
}

/// An additional, independent command queue (e.g. for the controller
/// thread). Created with [`Engine::new_sender`]; dropping it detaches it.
pub struct CommandSender {
    producer: rtrb::Producer<Command>,
}

impl CommandSender {
    pub fn send(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.producer.push(cmd).map_err(|_| EngineError::QueueFull)
    }
}

/// Lock-free tap of the master output (interleaved stereo `f32`), drained by
/// a non-real-time writer such as the recorder.
pub struct RecordTap {
    consumer: rtrb::Consumer<f32>,
    pub sample_rate: u32,
}

impl RecordTap {
    /// Moves all available samples into `out` (appending). Returns the
    /// number of samples appended (always a multiple of 2).
    pub fn drain_into(&mut self, out: &mut Vec<f32>) -> usize {
        let n = self.consumer.slots() & !1;
        if n == 0 {
            return 0;
        }
        let Ok(chunk) = self.consumer.read_chunk(n) else {
            return 0;
        };
        let (a, b) = chunk.as_slices();
        out.extend_from_slice(a);
        out.extend_from_slice(b);
        chunk.commit_all();
        n
    }

    /// Samples currently waiting.
    pub fn available(&self) -> usize {
        self.consumer.slots()
    }
}

impl Engine {
    /// Creates an engine. The processor is moved to the audio thread (see
    /// `od_audio_io::AudioHost::start`), the handle stays on the control side.
    pub fn new(config: EngineConfig) -> (Engine, EngineProcessor) {
        let config = config.validated();
        let (cmd_tx, cmd_rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        let (ev_tx, ev_rx) = rtrb::RingBuffer::new(EVENT_CAPACITY);
        let (gb_tx, gb_rx) = rtrb::RingBuffer::new(GARBAGE_CAPACITY);
        let (rec_tx, rec_rx) =
            rtrb::RingBuffer::new(config.sample_rate as usize * 2 * RECORD_SECONDS);
        let mut initial = EngineState {
            sample_rate: config.sample_rate,
            decks_count: config.decks,
            ..Default::default()
        };
        initial.channels.iter_mut().for_each(|c| {
            c.fader = 1.0;
            c.auto_gain = true;
            c.gain_to_master = 1.0;
        });
        let (st_in, st_out) = triple_buffer::triple_buffer(&initial);
        let active_senders = Arc::new(AtomicUsize::new(0));
        let processor = EngineProcessor::new(
            config,
            cmd_rx,
            st_in,
            ev_tx,
            gb_tx,
            rec_tx,
            active_senders.clone(),
        );
        let engine = Engine {
            config,
            commands: cmd_tx,
            state: st_out,
            events: ev_rx,
            garbage: gb_rx,
            record: Some(RecordTap {
                consumer: rec_rx,
                sample_rate: config.sample_rate,
            }),
            active_senders,
        };
        (engine, processor)
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Queues a command for the next audio block.
    pub fn send(&mut self, cmd: Command) -> Result<(), EngineError> {
        self.collect_garbage();
        self.commands.push(cmd).map_err(|_| EngineError::QueueFull)
    }

    /// Shorthand for `send(Command::Deck(deck, cmd))`.
    pub fn deck(&mut self, deck: usize, cmd: DeckCommand) -> Result<(), EngineError> {
        self.send(Command::Deck(deck, cmd))
    }

    /// Prepares a track for this engine (decode + resample). Slow; call from
    /// a worker thread, then send `DeckCommand::Load`.
    pub fn prepare_track(
        &self,
        id: od_core::TrackId,
        path: &std::path::Path,
        beatgrid: od_core::Beatgrid,
        cues: &[od_core::Cue],
        auto_gain_db: f32,
    ) -> Result<Arc<TrackData>, od_decode::DecodeError> {
        TrackData::load(
            id,
            path,
            self.config.sample_rate,
            beatgrid,
            cues,
            auto_gain_db,
        )
    }

    /// Creates an additional command queue for another thread.
    pub fn new_sender(&mut self) -> Result<CommandSender, EngineError> {
        if self.active_senders.load(Ordering::Acquire) >= MAX_SENDERS {
            return Err(EngineError::TooManySenders);
        }
        let (tx, rx) = rtrb::RingBuffer::new(COMMAND_CAPACITY);
        self.active_senders.fetch_add(1, Ordering::AcqRel);
        self.send(Command::AttachSender(rx))?;
        Ok(CommandSender { producer: tx })
    }

    /// Latest published state snapshot.
    pub fn state(&mut self) -> &EngineState {
        self.state.read()
    }

    /// Drains pending events.
    pub fn events(&mut self) -> impl Iterator<Item = Event> + '_ {
        std::iter::from_fn(move || self.events.pop().ok())
    }

    /// Takes the record tap (once). Recording starts when
    /// `Command::SetRecordTap(true)` is sent.
    pub fn take_record_tap(&mut self) -> Option<RecordTap> {
        self.record.take()
    }

    /// Frees objects the processor released (tracks, samples, queues).
    /// Called automatically by [`Engine::send`]; call periodically otherwise.
    pub fn collect_garbage(&mut self) {
        while let Ok(g) = self.garbage.pop() {
            drop(g);
        }
    }
}

/// Real-time processor. Lives on the audio thread; [`EngineProcessor::process`]
/// never allocates, locks, or blocks.
pub struct EngineProcessor {
    config: EngineConfig,
    sr: f64,
    commands: rtrb::Consumer<Command>,
    extra: [Option<rtrb::Consumer<Command>>; MAX_SENDERS],
    active_senders: Arc<AtomicUsize>,
    state_in: triple_buffer::Input<EngineState>,
    events: rtrb::Producer<Event>,
    garbage: rtrb::Producer<Garbage>,
    record: rtrb::Producer<f32>,
    record_enabled: bool,
    record_dropped: u64,
    leaked: u64,

    decks: Vec<Deck>,
    channels: Vec<Channel>,
    /// Beat FX slots, `unit * BEAT_FX_SLOTS + slot`.
    beat_fx: Vec<FxUnit>,
    beat_fx_multi: [bool; BEAT_FX_UNITS],
    pad_fx: Vec<FxUnit>,
    sampler: Sampler,
    input: crate::input::AuxInput,
    limiter: Limiter,
    limiter_on: bool,
    master_meter: Meter,
    hp_meter: Meter,

    deck_bufs: Vec<Vec<f32>>,
    master_buf: Vec<f32>,
    cue_buf: Vec<f32>,
    sampler_buf: Vec<f32>,
    input_buf: Vec<f32>,
    booth_buf: Vec<f32>,

    quantize: bool,
    explicit_master: Option<usize>,
    master: Option<usize>,
    master_bpm: f64,
    free_beat: f64,
    crossfader: f32,
    xf_curve: CrossfaderCurve,
    master_level: f32,
    master_gain: Smoothed,
    hp_mix: f32,
    hp_level: f32,
    hp_gain: Smoothed,
    sampler_level: f32,
    sampler_cue: bool,
    booth_level: f32,
    booth_gain: Smoothed,
    master_cue: bool,
    frames_total: u64,
    state: EngineState,
}

impl EngineProcessor {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: EngineConfig,
        commands: rtrb::Consumer<Command>,
        state_in: triple_buffer::Input<EngineState>,
        events: rtrb::Producer<Event>,
        garbage: rtrb::Producer<Garbage>,
        record: rtrb::Producer<f32>,
        active_senders: Arc<AtomicUsize>,
    ) -> Self {
        let sr = config.sample_rate;
        let n = config.decks;
        let block = config.max_block * 2;
        let ramp = (sr as f32 * 0.01) as u32;
        Self {
            config,
            sr: sr as f64,
            commands,
            extra: std::array::from_fn(|_| None),
            active_senders,
            state_in,
            events,
            garbage,
            record,
            record_enabled: false,
            record_dropped: 0,
            leaked: 0,
            decks: (0..n)
                .map(|i| Deck::new(i, sr, config.max_block, config.profile))
                .collect(),
            channels: (0..n)
                .map(|i| Channel::new(sr, 0x9E37_79B9 ^ (i as u32 + 1)))
                .collect(),
            beat_fx_multi: [false; BEAT_FX_UNITS],
            beat_fx: (0..BEAT_FX_UNITS * BEAT_FX_SLOTS)
                .map(|_| FxUnit::new(sr, BEAT_FX_SECONDS, FxTarget::Master))
                .collect(),
            pad_fx: (0..n)
                .map(|i| FxUnit::new(sr, PAD_FX_SECONDS, FxTarget::Channel(i)))
                .collect(),
            sampler: Sampler::new(sr),
            input: crate::input::AuxInput::new(sr),
            limiter: Limiter::new(sr),
            limiter_on: true,
            master_meter: Meter::new(sr),
            hp_meter: Meter::new(sr),
            deck_bufs: (0..n).map(|_| vec![0.0; block]).collect(),
            master_buf: vec![0.0; block],
            cue_buf: vec![0.0; block],
            sampler_buf: vec![0.0; block],
            input_buf: vec![0.0; block],
            booth_buf: vec![0.0; block],
            quantize: true,
            explicit_master: None,
            master: None,
            master_bpm: 120.0,
            free_beat: 0.0,
            crossfader: 0.0,
            xf_curve: CrossfaderCurve::default(),
            master_level: 1.0,
            master_gain: Smoothed::new(1.0, ramp),
            hp_mix: 0.0,
            hp_level: 1.0,
            hp_gain: Smoothed::new(1.0, ramp),
            sampler_level: 1.0,
            sampler_cue: false,
            booth_level: 1.0,
            booth_gain: Smoothed::new(1.0, ramp),
            master_cue: false,
            frames_total: 0,
            state: EngineState {
                sample_rate: sr,
                decks_count: n,
                ..Default::default()
            },
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Renders `frames` frames. `outputs.master` (and `outputs.headphones`,
    /// if given) must hold at least `frames * 2` samples.
    pub fn process(&mut self, frames: usize, outputs: &mut Outputs<'_>) {
        self.drain_commands();
        let max = self.config.max_block;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(max);
            self.render_block(n);
            let m = &mut outputs.master[done * 2..(done + n) * 2];
            m.copy_from_slice(&self.master_buf[..n * 2]);
            if let Some(hp) = outputs.headphones.as_deref_mut() {
                hp[done * 2..(done + n) * 2].copy_from_slice(&self.cue_buf[..n * 2]);
            }
            if let Some(b) = outputs.booth.as_deref_mut() {
                b[done * 2..(done + n) * 2].copy_from_slice(&self.booth_buf[..n * 2]);
            }
            done += n;
        }
        self.publish();
    }

    // ----- commands ------------------------------------------------------

    fn drain_commands(&mut self) {
        let mut budget = COMMAND_CAPACITY;
        while budget > 0 {
            match self.commands.pop() {
                Ok(c) => self.handle(c),
                Err(_) => break,
            }
            budget -= 1;
        }
        for i in 0..MAX_SENDERS {
            let mut budget = COMMAND_CAPACITY;
            while budget > 0 {
                let cmd = match self.extra[i].as_mut() {
                    Some(q) => q.pop().ok(),
                    None => None,
                };
                match cmd {
                    Some(c) => self.handle(c),
                    None => break,
                }
                budget -= 1;
            }
            let abandoned = self.extra[i]
                .as_ref()
                .is_some_and(|q| q.is_abandoned() && q.is_empty());
            if abandoned && let Some(q) = self.extra[i].take() {
                let mut ctx = self.ctx();
                ctx.dispose(Garbage::Commands(q));
                self.active_senders.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    fn ctx(&mut self) -> RtCtx<'_> {
        RtCtx {
            quantize: self.quantize,
            events: &mut self.events,
            garbage: &mut self.garbage,
            leaked: &mut self.leaked,
        }
    }

    fn handle(&mut self, cmd: Command) {
        let mut ctx = RtCtx {
            quantize: self.quantize,
            events: &mut self.events,
            garbage: &mut self.garbage,
            leaked: &mut self.leaked,
        };
        match cmd {
            Command::Deck(d, c) => {
                if let Some(deck) = self.decks.get_mut(d) {
                    deck.apply(c, &mut ctx);
                } else if let DeckCommand::Load(t) = c {
                    ctx.dispose_track(t);
                }
            }
            Command::Channel(ch, c) => {
                if let Some(chn) = self.channels.get_mut(ch) {
                    chn.apply(c);
                }
            }
            Command::Mixer(m) => match m {
                MixerCommand::SetCrossfader(x) => self.crossfader = x.clamp(-1.0, 1.0),
                MixerCommand::SetCrossfaderCurve(c) => self.xf_curve = c,
                MixerCommand::SetMasterLevel(l) => self.master_level = l.clamp(0.0, 1.5),
                MixerCommand::SetHeadphoneMix(x) => self.hp_mix = x.clamp(0.0, 1.0),
                MixerCommand::SetHeadphoneLevel(l) => self.hp_level = l.clamp(0.0, 1.5),
                MixerCommand::SetSamplerLevel(l) => self.sampler_level = l.clamp(0.0, 1.5),
                MixerCommand::SetSamplerCue(on) => self.sampler_cue = on,
                MixerCommand::SetBoothLevel(l) => self.booth_level = l.clamp(0.0, 1.5),
                MixerCommand::SetMasterCue(on) => self.master_cue = on,
                MixerCommand::SetLimiter(on) => self.limiter_on = on,
                MixerCommand::ResetClipIndicators => {
                    self.master_meter.reset_clip();
                    self.hp_meter.reset_clip();
                    self.channels.iter_mut().for_each(|c| c.meter.reset_clip());
                }
            },
            Command::BeatFx(u, c) => {
                if u < BEAT_FX_UNITS {
                    if let BeatFxCommand::SetTarget(t) = c {
                        // The slots of a unit share its insert point.
                        for s in 1..BEAT_FX_SLOTS {
                            self.beat_fx[u * BEAT_FX_SLOTS + s].apply(BeatFxCommand::SetTarget(t));
                        }
                    }
                    self.beat_fx[u * BEAT_FX_SLOTS].apply(c);
                }
            }
            Command::BeatFxSlot { unit, slot, cmd } => {
                if unit < BEAT_FX_UNITS && (1..BEAT_FX_SLOTS).contains(&slot) && !matches!(cmd, BeatFxCommand::SetTarget(_)) {
                    self.beat_fx[unit * BEAT_FX_SLOTS + slot].apply(cmd);
                }
            }
            Command::SetBeatFxMulti(u, on) => {
                if u < BEAT_FX_UNITS {
                    self.beat_fx_multi[u] = on;
                }
            }
            Command::PadFx(ch, c) => {
                if let Some(unit) = self.pad_fx.get_mut(ch) {
                    unit.apply_pad(c);
                }
            }
            Command::Sampler(c) => self.sampler.apply(c, &mut ctx),
            Command::SetQuantize(q) => self.quantize = q,
            Command::SetMasterDeck(m) => {
                self.explicit_master = m.filter(|&d| d < self.decks.len());
            }
            Command::SetRecordTap(on) => self.record_enabled = on,
            Command::Input(c) => {
                let mut ctx = RtCtx {
                    quantize: self.quantize,
                    events: &mut self.events,
                    garbage: &mut self.garbage,
                    leaked: &mut self.leaked,
                };
                self.input.handle(c, &mut ctx);
            }
            Command::AttachSender(q) => {
                if let Some(slot) = self.extra.iter_mut().find(|s| s.is_none()) {
                    *slot = Some(q);
                } else {
                    ctx.dispose(Garbage::Commands(q));
                    self.active_senders.fetch_sub(1, Ordering::AcqRel);
                }
            }
        }
    }

    // ----- rendering -----------------------------------------------------

    fn select_master(&mut self) {
        let decks = &self.decks;
        let usable = |d: usize| decks[d].is_loaded() && decks[d].track_bpm().is_some();
        let new = if let Some(m) = self.explicit_master.filter(|&m| usable(m)) {
            Some(m)
        } else if let Some(m) = self.master.filter(|&m| usable(m) && decks[m].is_playing()) {
            Some(m)
        } else if let Some(m) = (0..decks.len())
            .find(|&d| usable(d) && decks[d].is_playing() && !decks[d].sync_enabled())
        {
            Some(m)
        } else if let Some(m) = (0..decks.len()).find(|&d| usable(d) && decks[d].is_playing()) {
            Some(m)
        } else {
            self.master.filter(|&m| usable(m))
        };
        if new != self.master {
            self.master = new;
            let _ = self.events.push(Event::MasterDeckChanged { deck: new });
        }
        if let Some(bpm) = self.master.and_then(|m| self.decks[m].bpm()) {
            self.master_bpm = bpm;
        }
    }

    fn render_block(&mut self, n: usize) {
        self.select_master();
        let sr = self.sr;

        // Beat clock for sync and FX.
        let master_info = self.master.map(|m| {
            let d = &self.decks[m];
            (d.current_beat().unwrap_or(0.0), d.is_playing())
        });
        let bpf = self.master_bpm / 60.0 / sr;
        let clock = match master_info {
            Some((beat, true)) => BeatClock {
                bpm: self.master_bpm,
                beat,
                beats_per_frame: bpf,
            },
            _ => BeatClock {
                bpm: self.master_bpm,
                beat: self.free_beat,
                beats_per_frame: bpf,
            },
        };
        self.free_beat = clock.beat + bpf * n as f64;
        let master_key = self.master.and_then(|m| self.decks[m].key_pitch());
        let target = master_info.map(|(beat, playing)| SyncTarget {
            bpm: self.master_bpm,
            beat,
            playing,
            key: master_key,
        });
        // Per-channel FX clocks: the channel's own deck if it plays a track
        // with a grid, else the master clock.
        let mut clocks = [clock; MAX_DECKS];
        for (i, d) in self.decks.iter().enumerate() {
            if d.is_playing()
                && let (Some(bpm), Some(beat)) = (d.bpm(), d.current_beat())
                && bpm > 1.0
            {
                clocks[i] = BeatClock {
                    bpm,
                    beat,
                    beats_per_frame: bpm / 60.0 / sr,
                };
            }
        }

        // Decks.
        {
            let mut ctx = RtCtx {
                quantize: self.quantize,
                events: &mut self.events,
                garbage: &mut self.garbage,
                leaked: &mut self.leaked,
            };
            for (i, deck) in self.decks.iter_mut().enumerate() {
                let is_master = self.master == Some(i);
                deck.prepare(n, if is_master { None } else { target }, is_master);
                deck.render(&mut self.deck_bufs[i], n, &mut ctx);
            }
        }

        // Mixer.
        let len = n * 2;
        let master_buf = &mut self.master_buf[..len];
        let cue_buf = &mut self.cue_buf[..len];
        master_buf.fill(0.0);
        cue_buf.fill(0.0);
        let xf = crossfader_gains(self.crossfader, self.xf_curve);
        let master_lin = level_gain(self.master_level);
        self.master_gain.set(master_lin);
        let block_s = n as f32 / sr as f32;
        let contrib_a = (-block_s / 0.25).exp();
        for (i, ch) in self.channels.iter_mut().enumerate() {
            let buf = &mut self.deck_bufs[i][..len];
            let auto_gain = self.decks[i].track().map_or(0.0, |t| t.auto_gain_db);
            ch.update_gains(auto_gain, xf);
            let ch_clock = clocks[i];
            ch.process_pre(buf, &ch_clock);
            for (k, unit) in self.beat_fx.iter_mut().enumerate() {
                let live = k % BEAT_FX_SLOTS == 0 || self.beat_fx_multi[k / BEAT_FX_SLOTS];
                if live && unit.target() == FxTarget::Channel(i) {
                    unit.process(buf, &ch_clock);
                }
            }
            self.pad_fx[i].process(buf, &ch_clock);
            if ch.cue {
                for (c, s) in cue_buf.iter_mut().zip(buf.iter()) {
                    *c += *s;
                }
            }
            let mut sum_sq = 0.0f32;
            for (m, f) in master_buf
                .as_chunks_mut::<2>()
                .0
                .iter_mut()
                .zip(buf.chunks_exact(2))
            {
                let g = ch.post_gain.next();
                let l = f[0] * g;
                let r = f[1] * g;
                sum_sq += l * l + r * r;
                m[0] += l;
                m[1] += r;
            }
            let ms = sum_sq / len.max(1) as f32;
            ch.contribution = ch.contribution * contrib_a + ms * (1.0 - contrib_a);
            ch.gain_to_master = ch.post_gain.target() * master_lin;
        }

        let sampler_buf = &mut self.sampler_buf[..len];
        sampler_buf.fill(0.0);
        self.sampler
            .mix_into(sampler_buf, level_gain(self.sampler_level), &clock);
        for (k, unit) in self.beat_fx.iter_mut().enumerate() {
            let live = k % BEAT_FX_SLOTS == 0 || self.beat_fx_multi[k / BEAT_FX_SLOTS];
            if live && unit.target() == FxTarget::Sampler {
                unit.process(sampler_buf, &clock);
            }
        }
        for (m, s) in master_buf.iter_mut().zip(sampler_buf.iter()) {
            *m += *s;
        }
        if self.sampler_cue {
            for (c, s) in cue_buf.iter_mut().zip(sampler_buf.iter()) {
                *c += *s;
            }
        }
        // Mic / aux after the decks and sampler (talkover ducks them).
        self.input
            .process(&mut self.input_buf[..len], master_buf, cue_buf);
        for (k, unit) in self.beat_fx.iter_mut().enumerate() {
            let live = k % BEAT_FX_SLOTS == 0 || self.beat_fx_multi[k / BEAT_FX_SLOTS];
            if live && unit.target() == FxTarget::Master {
                unit.process(master_buf, &clock);
            }
        }
        // Booth: master bus before master level / limiter.
        self.booth_gain.set(level_gain(self.booth_level));
        for (b, m) in self.booth_buf[..len]
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(master_buf.chunks_exact(2))
        {
            let g = self.booth_gain.next();
            b[0] = soft_clip(m[0] * g);
            b[1] = soft_clip(m[1] * g);
        }
        for f in master_buf.as_chunks_mut::<2>().0 {
            let g = self.master_gain.next();
            f[0] *= g;
            f[1] *= g;
        }
        if self.limiter_on {
            self.limiter.process(master_buf);
        }
        self.master_meter.process(master_buf);

        if self.record_enabled {
            let (pushed, _) = self.record.push_partial_slice(master_buf);
            self.record_dropped += (len - pushed.len()) as u64;
        }

        if self.master_cue {
            for (c, m) in cue_buf.iter_mut().zip(master_buf.iter()) {
                *c += *m;
            }
        }

        // Headphones: cue bus / master blend.
        let ang = self.hp_mix * std::f32::consts::FRAC_PI_2;
        let (cg, mg) = (ang.cos(), ang.sin());
        self.hp_gain.set(level_gain(self.hp_level));
        for (c, m) in cue_buf
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(master_buf.chunks_exact(2))
        {
            let g = self.hp_gain.next();
            c[0] = soft_clip((c[0] * cg + m[0] * mg) * g);
            c[1] = soft_clip((c[1] * cg + m[1] * mg) * g);
        }
        self.hp_meter.process(cue_buf);

        self.frames_total += n as u64;
    }

    fn publish(&mut self) {
        let s = &mut self.state;
        s.frames = self.frames_total;
        for (i, d) in self.decks.iter().enumerate() {
            s.decks[i] = d.state(self.master == Some(i));
        }
        let mut audible = AudibleState::default();
        let master_lin = level_gain(self.master_level);
        let mut best: Option<(usize, f32)> = None;
        for (i, c) in self.channels.iter().enumerate() {
            s.channels[i] = c.state();
            s.channels[i].pad_fx = self.pad_fx[i].pad_state();
            let level = c.contribution.max(0.0).sqrt() * master_lin;
            audible.level[i] = level;
            let playing = self.decks[i].is_playing() && self.decks[i].is_loaded();
            audible.audible[i] = playing && level > AUDIBLE_THRESHOLD;
            if audible.audible[i] && best.is_none_or(|(_, l)| level > l) {
                best = Some((i, level));
            }
        }
        audible.dominant = best.map(|(i, _)| i);
        s.audible = audible;
        s.master_meter = self.master_meter.reading();
        s.headphone_meter = self.hp_meter.reading();
        s.limiter_reduction_db = if self.limiter_on {
            self.limiter.gain_reduction_db()
        } else {
            0.0
        };
        s.crossfader = self.crossfader;
        s.crossfader_curve = self.xf_curve;
        s.master_level = self.master_level;
        s.headphone_mix = self.hp_mix;
        s.headphone_level = self.hp_level;
        s.sampler_level = self.sampler_level;
        s.sampler_cue = self.sampler_cue;
        s.booth_level = self.booth_level;
        s.master_cue = self.master_cue;
        s.quantize = self.quantize;
        s.master_deck = self.master;
        s.master_bpm = self.master_bpm;
        for u in 0..BEAT_FX_UNITS {
            s.beat_fx[u] = self.beat_fx[u * BEAT_FX_SLOTS].state();
            for k in 1..BEAT_FX_SLOTS {
                s.beat_fx_slots[u][k - 1] = self.beat_fx[u * BEAT_FX_SLOTS + k].state();
            }
        }
        s.beat_fx_multi = self.beat_fx_multi;
        s.sampler = self.sampler.state();
        s.sequencer = self.sampler.sequencer();
        s.input = self.input.state();
        s.record_tap = self.record_enabled;
        s.record_dropped = self.record_dropped;
        s.garbage_leaked = self.leaked;
        *self.state_in.input_buffer_mut() = *s;
        self.state_in.publish();
    }
}
