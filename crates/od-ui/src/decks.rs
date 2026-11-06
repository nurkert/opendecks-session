//! Decks: loading tracks (decode and stems off the UI thread), stem
//! separation, the deck view models and the deck controls.
//!
//! The deck views are persistent models: every frame the engine state is
//! projected into plain [`DeckFields`]; a row is written only when its
//! fields or its per-track models changed. The zoomed waveforms scroll
//! smoothly: tiles one lane wide are rendered once and the UI translates
//! them, extrapolating the playhead at the display rate from the position
//! and speed published here (see [`LaneScroll`]).

use crate::app::{App, handler};
use crate::models::{Rows, model};
use crate::player::{cue_marks, phrase_marks};
use crate::{CueMark, DeckView, HotCueSlot, LaneMotion, PhraseMark, engine, format, player, render};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_controller::PadMode;
use od_core::{Beatgrid, CueKind, TrackId, Waveform};
use od_engine::{Command, DeckCommand, DeckState, EngineState, TempoRange};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const DECKS: usize = 4;
/// The hidden deck the Prepare preview plays on.
pub(crate) const PREVIEW_DECK: usize = 3;
/// Seconds of audio shown across the scrolling waveform.
const DETAIL_SPAN: f64 = 8.0;
/// Loudness target for auto-gain.
const TARGET_LUFS: f32 = -10.0;

/// View data of a loaded track.
#[derive(Clone)]
pub(crate) struct Loaded {
    pub id: TrackId,
    pub title: String,
    pub artist: String,
    key: String,
    waveform: Waveform,
    grid: Beatgrid,
    overview: Image,
    cues: Vec<CueMark>,
    labels: [String; 8],
    phrases: Vec<od_core::Phrase>,
    phrase_marks: Vec<PhraseMark>,
    /// What a reload with stems needs.
    source: TrackSource,
}

/// File and analysis of a loaded track, kept to reload it with stems.
#[derive(Clone)]
struct TrackSource {
    path: PathBuf,
    hash: od_core::ContentHash,
    cues: Vec<od_core::Cue>,
    gain: f32,
}

struct Prepared {
    deck: usize,
    data: Arc<od_engine::TrackData>,
    play_from: Option<f64>,
    /// Replaces the data of the track already on `deck` without a gap.
    swap: Option<Swap>,
}

/// Why a loaded track's data is swapped in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Swap {
    /// Stems were separated.
    Stems,
    /// Analysis (beatgrid, loudness) arrived after loading.
    Analysis,
}

pub(crate) struct Decks {
    /// Decks shown in Perform (2 or 4).
    pub deck_count: usize,
    loaded: [Option<Loaded>; DECKS],
    /// View data of tracks being prepared, moved to `loaded` on arrival.
    pending: [Option<Loaded>; DECKS],
    prepared_tx: Sender<Prepared>,
    prepared_rx: Receiver<Prepared>,
    loaded_from_stick: bool,
    /// Auto-loop size per deck, in beats.
    pub loop_size: [f64; DECKS],
    pub shift: [bool; DECKS],
    pub pad_mode: [PadMode; DECKS],
    stem_cache: od_stems::StemCache,
    #[cfg(feature = "separation")]
    stem_worker: Option<od_stems::StemWorker>,
    /// Separation progress (0..1) of queued or running tracks.
    stem_progress: HashMap<TrackId, f32>,
    pub views: DeckViews,
    /// Waveform zoom per deck (1 = 8 s across, smaller = closer).
    pub zoom: [f64; DECKS],
    scroll: [LaneScroll; DECKS],
    motion: Rc<VecModel<LaneMotion>>,
}

impl Default for Decks {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            deck_count: 2,
            loaded: Default::default(),
            pending: Default::default(),
            prepared_tx: tx,
            prepared_rx: rx,
            loaded_from_stick: false,
            loop_size: [4.0; DECKS],
            shift: [false; DECKS],
            pad_mode: [PadMode::HotCue; DECKS],
            stem_cache: od_stems::StemCache::open_default(),
            #[cfg(feature = "separation")]
            stem_worker: None,
            stem_progress: HashMap::new(),
            views: DeckViews::default(),
            zoom: [1.0; DECKS],
            scroll: Default::default(),
            motion: Rc::new(VecModel::from(vec![LaneMotion::default(); DECKS])),
        }
    }
}

/// Smooth scrolling of one zoomed waveform lane: the rendered tiles and the
/// motion last published to the UI.
#[derive(Default)]
struct LaneScroll {
    /// What the tiles were rendered for: track, span (ms), size, phrases.
    key: Option<(TrackId, i64, u32, u32, bool)>,
    tiles: HashMap<i64, Image>,
    first: Option<i64>,
    sent_pos: f64,
    sent_rate: f64,
    sent_at: Option<Instant>,
    loaded: bool,
}

/// Inputs for one lane's scroll update.
struct ScrollIn<'a> {
    meta: Option<&'a Loaded>,
    position: f64,
    /// Seconds of track per real second (0 when paused or scratching).
    speed: f64,
    span: f64,
    width: u32,
    height: u32,
    phrases: bool,
    now: Instant,
}

impl LaneScroll {
    /// Brings the tiles up to date and returns a new motion row when the UI
    /// needs one (new tiles, speed change, drift, or a periodic resync).
    fn update(&mut self, i: &ScrollIn) -> Option<LaneMotion> {
        let Some(m) = i.meta else {
            if !self.loaded {
                return None;
            }
            *self = Self::default();
            return Some(LaneMotion::default());
        };
        let key = (m.id, (i.span * 1000.0).round() as i64, i.width, i.height, i.phrases);
        let mut force = !self.loaded;
        if self.key != Some(key) {
            self.key = Some(key);
            self.tiles.clear();
            self.first = None;
            force = true;
        }
        let rate = i.speed / 1000.0;
        let elapsed_ms = self.sent_at.map(|t| i.now.duration_since(t).as_secs_f64() * 1000.0).unwrap_or(0.0);
        let predicted = self.sent_pos + self.sent_rate * elapsed_ms;
        let drift = i.position - predicted;
        // While playing steadily, glide towards the engine position instead
        // of jumping to it: engine updates arrive in audio-block steps.
        let pos = if rate != 0.0 && self.sent_rate == rate && drift.abs() < 0.05 { predicted + drift * 0.15 } else { i.position };
        let first = ((pos - i.span / 2.0) / i.span).floor() as i64;
        let tiles_moved = self.first != Some(first);
        if tiles_moved || force {
            self.tiles.retain(|k, _| (first..first + 3).contains(k));
            for k in first..first + 3 {
                self.tiles.entry(k).or_insert_with(|| {
                    render::detail_tile(&m.waveform, &m.grid, if i.phrases { &m.phrases } else { &[] }, k as f64 * i.span, i.span, i.width, i.height)
                });
            }
            self.first = Some(first);
        }
        let resync = elapsed_ms > 500.0;
        if !(force || tiles_moved || rate != self.sent_rate || drift.abs() > 0.004 || resync) {
            return None;
        }
        self.loaded = true;
        self.sent_pos = pos;
        self.sent_rate = rate;
        self.sent_at = Some(i.now);
        let tile = |k: i64| self.tiles.get(&k).cloned().unwrap_or_default();
        Some(LaneMotion {
            loaded: true,
            pos: pos as f32,
            rate: rate as f32,
            span: i.span as f32,
            tile0: tile(first),
            tile1: tile(first + 1),
            tile2: tile(first + 2),
            t0: (first as f64 * i.span) as f32,
            t1: ((first + 1) as f64 * i.span) as f32,
            t2: ((first + 2) as f64 * i.span) as f32,
        })
    }
}

impl Decks {
    /// Tracks currently loaded on decks (preview deck excluded), 0-based decks.
    pub fn loaded_decks(&self) -> Vec<(TrackId, usize)> {
        (0..self.deck_count).filter_map(|d| self.loaded[d].as_ref().map(|l| (l.id, d))).collect()
    }

    /// Whether loaded tracks came from a stick (ids then refer to it).
    pub fn source_is_stick(&self) -> bool {
        self.loaded_from_stick
    }

    pub fn loaded_id(&self, deck: usize) -> Option<TrackId> {
        self.loaded.get(deck).and_then(|l| l.as_ref()).map(|l| l.id)
    }

    pub fn loaded(&self, deck: usize) -> Option<&Loaded> {
        self.loaded.get(deck).and_then(|l| l.as_ref())
    }
}

// ── Loading ─────────────────────────────────────────────────────────────

/// Loads `track` (of the current source) into `deck` in the background.
pub(crate) fn load_track(app: &App, deck: usize, track: TrackId, play_from: Option<f64>) {
    if !engine::ensure_engine(app) {
        engine::show_message(app);
        return;
    }
    let rate = app.engine.borrow().sample_rate();
    let st = app.state.borrow();
    let Ok(t) = st.src().track(track) else { return };
    let analysis = st.src().analysis(track).unwrap_or_default();
    let cues = st.src().cues(track).unwrap_or_default();
    let scale = app.ui.window().scale_factor();
    let gain = analysis.loudness_lufs.map(|l| (TARGET_LUFS - l).clamp(-12.0, 12.0)).unwrap_or(0.0);
    let source = TrackSource { path: st.resolve(&t.path), hash: t.content_hash.clone(), cues: cues.clone(), gain };
    let meta = Loaded {
        id: track,
        title: t.tags.title.clone(),
        artist: t.tags.artist.clone(),
        key: st.key_text(&t),
        overview: render::overview(&analysis.waveform, &analysis.beatgrid, t.duration, (900.0 * scale) as u32, (30.0 * scale) as u32),
        waveform: analysis.waveform.clone(),
        grid: analysis.beatgrid.clone(),
        cues: cue_marks(&cues, t.duration, false),
        labels: std::array::from_fn(|s| cues.iter().find(|c| c.kind == CueKind::Hot(s as u8)).map(|c| c.label.clone()).unwrap_or_default()),
        phrases: analysis.phrases.clone(),
        phrase_marks: phrase_marks(&analysis.phrases, &analysis.beatgrid, t.duration),
        source: source.clone(),
    };
    let read_only = st.read_only();
    drop(st);
    app.ui.set_engine_message(format!("Loading {} …", meta.title).into());
    let (cache, tx) = {
        let mut d = app.decks.borrow_mut();
        d.pending[deck] = Some(meta);
        d.loaded_from_stick = read_only;
        (d.stem_cache.clone(), d.prepared_tx.clone())
    };
    prepare_in_background(cache, tx, deck, track, source, rate, analysis.beatgrid, play_from, None);
}

/// Decodes a track (with its cached stems, if any) off the UI thread.
#[allow(clippy::too_many_arguments)]
fn prepare_in_background(
    cache: od_stems::StemCache,
    tx: Sender<Prepared>,
    deck: usize,
    track: TrackId,
    src: TrackSource,
    rate: u32,
    grid: Beatgrid,
    play_from: Option<f64>,
    swap: Option<Swap>,
) {
    std::thread::spawn(move || match od_stems::load_track_data(&cache, track, &src.path, Some(&src.hash), rate, grid, &src.cues, src.gain) {
        Ok(data) => {
            let _ = tx.send(Prepared { deck, data, play_from, swap });
        }
        Err(e) => tracing::warn!(error = %e, "track load failed"),
    });
}

/// Frame tick: prepared tracks → engine.
pub(crate) fn receive_prepared(app: &App) {
    let prepared: Vec<Prepared> = app.decks.borrow().prepared_rx.try_iter().collect();
    for pr in prepared {
        let (deck, play_from) = (pr.deck, pr.play_from);
        if let Some(why) = pr.swap {
            // Same track still on the deck: swap without a gap.
            if app.decks.borrow().loaded_id(deck) == Some(pr.data.id) {
                engine::deck_cmd(app, deck, DeckCommand::SwapTrackData(pr.data));
                if why == Swap::Stems {
                    app.status(format!("Stems ready on deck {}", deck + 1));
                }
            }
            continue;
        }
        let needs_stems = !pr.data.has_stems();
        let meta = app.decks.borrow_mut().pending[deck].take().filter(|m| m.id == pr.data.id);
        let Some(meta) = meta else { continue };
        let title = meta.title.clone();
        if needs_stems && deck != PREVIEW_DECK {
            request_stems(app, meta.id, &meta.source);
        }
        app.decks.borrow_mut().loaded[deck] = Some(meta);
        crate::browser::refresh_tracks(app);
        engine::deck_cmd(app, deck, DeckCommand::Load(pr.data));
        if let Some(t) = play_from {
            engine::deck_cmd(app, deck, DeckCommand::Seek { seconds: t });
            engine::deck_cmd(app, deck, DeckCommand::Play);
        }
        engine::show_message(app);
        app.status(format!("Loaded {title} on deck {}", deck + 1));
    }
}

/// Double-click in the library: load into the first deck that is not
/// playing (Perform) or start the preview (Prepare).
pub(crate) fn load_selected_into_free_deck(app: &App) {
    let Some(track) = app.state.borrow().player.current else { return };
    if app.state.borrow().config.kiosk {
        app.ui.set_browser_open(false);
    }
    if app.ui.get_mode() == "prepare" {
        player::toggle_preview(app);
        return;
    }
    let playing: Vec<bool> = match engine::snapshot(app) {
        Some(s) => s.decks.iter().map(|d| d.playing).collect(),
        None => vec![false; DECKS],
    };
    let deck = {
        let d = app.decks.borrow();
        free_deck(d.deck_count, &playing, |i| d.loaded[i].is_some())
    };
    load_track(app, deck, track, None);
}

/// First deck that is neither playing nor loaded, else the first that is
/// not playing, else deck 1.
pub(crate) fn free_deck(count: usize, playing: &[bool], loaded: impl Fn(usize) -> bool) -> usize {
    let idle = |d: usize| !playing.get(d).copied().unwrap_or(false);
    (0..count).find(|&d| idle(d) && !loaded(d)).or_else(|| (0..count).find(|&d| idle(d))).unwrap_or(0)
}

/// LOAD on a deck: the selected track (an Explorer file is added to the
/// collection first).
pub(crate) fn load_selected(app: &App, deck: usize) {
    if crate::explorer::load_selection(app, Some(deck)) {
        return;
    }
    if let Some(t) = app.state.borrow().player.current {
        load_track(app, deck, t, None);
    }
}

/// OPENDECKS_AUTOLOAD="1:12,2:13" loads tracks on start (standalone
/// setups and testing).
pub(crate) fn autoload(app: &App) {
    let Ok(spec) = std::env::var("OPENDECKS_AUTOLOAD") else { return };
    for (d, t) in parse_autoload(&spec) {
        load_track(app, d - 1, t, None);
        if app.state.borrow().config.kiosk {
            app.ui.set_browser_open(false);
        }
    }
}

/// "1:12, 2:13" → [(1, track 12), (2, track 13)]; invalid parts are skipped.
pub(crate) fn parse_autoload(spec: &str) -> Vec<(usize, TrackId)> {
    spec.split(',')
        .filter_map(|part| part.split_once(':'))
        .filter_map(|(d, t)| Some((d.trim().parse::<usize>().ok()?, TrackId(t.trim().parse::<i64>().ok()?))))
        .filter(|(d, _)| (1..=DECKS).contains(d))
        .collect()
}

/// After analysis: tracks that were loaded before their analysis existed
/// (e.g. just added from the Explorer) get their waveform, grid and phrases,
/// and the engine gets the analysed data (beatgrid for BPM / sync, gain)
/// swapped in without a gap.
pub(crate) fn refresh_analysis(app: &App) {
    let rate = app.engine.borrow().sample_rate();
    let st = app.state.borrow();
    let mut d = app.decks.borrow_mut();
    if d.loaded_from_stick {
        return;
    }
    let scale = app.ui.window().scale_factor();
    let mut swaps = Vec::new();
    for i in 0..DECKS {
        let Some(m) = d.loaded[i].as_mut().filter(|m| m.waveform.columns.is_empty()) else { continue };
        let a = st.lib.analysis(m.id).unwrap_or_default();
        if a.waveform.columns.is_empty() {
            continue;
        }
        let duration = st.lib.track(m.id).map(|t| t.duration).unwrap_or(0.0);
        m.overview = render::overview(&a.waveform, &a.beatgrid, duration, (900.0 * scale) as u32, (30.0 * scale) as u32);
        m.phrase_marks = phrase_marks(&a.phrases, &a.beatgrid, duration);
        m.waveform = a.waveform;
        m.grid = a.beatgrid;
        m.phrases = a.phrases;
        if let Some(k) = st.lib.track(m.id).ok().and_then(|t| t.key) {
            m.key = k.format(st.settings.key_notation());
        }
        m.source.gain = a.loudness_lufs.map(|l| (TARGET_LUFS - l).clamp(-12.0, 12.0)).unwrap_or(0.0);
        swaps.push((i, m.id, m.source.clone(), m.grid.clone()));
        // Rebuild the per-track view parts on the next frame.
        d.views.lanes[i].track = None;
    }
    let (cache, tx) = (d.stem_cache.clone(), d.prepared_tx.clone());
    drop(d);
    drop(st);
    for (deck, id, src, grid) in swaps {
        prepare_in_background(cache.clone(), tx.clone(), deck, id, src, rate, grid, None, Some(Swap::Analysis));
    }
}

// ── Stems ───────────────────────────────────────────────────────────────

/// Queues stem separation of a loaded track (desktop profile only).
#[cfg(feature = "separation")]
fn request_stems(app: &App, track: TrackId, src: &TrackSource) {
    if app.state.borrow().config.lite {
        return;
    }
    let mut d = app.decks.borrow_mut();
    if d.stem_progress.contains_key(&track) {
        return;
    }
    let cache = d.stem_cache.clone();
    let worker = d.stem_worker.get_or_insert_with(|| od_stems::StemWorker::spawn(cache, od_stems::StemWorkerOptions::default()));
    worker.enqueue(od_stems::StemJob { track, path: src.path.clone(), hash: src.hash.clone() });
    worker.prioritize(track);
    d.stem_progress.insert(track, 0.0);
}

#[cfg(not(feature = "separation"))]
fn request_stems(_app: &App, _track: TrackId, _src: &TrackSource) {}

/// Separation events → progress, status and in-place stem swaps.
#[cfg(feature = "separation")]
pub(crate) fn poll_stems(app: &App) {
    use od_stems::StemEvent;
    let events: Vec<StemEvent> = app.decks.borrow().stem_worker.as_ref().map(|w| w.events().try_iter().collect()).unwrap_or_default();
    for ev in events {
        match ev {
            StemEvent::ModelDownload { done, total } => {
                let pct = if total > 0 { done * 100 / total } else { 0 };
                app.status(format!("Downloading the stem model … {pct} %"));
            }
            StemEvent::Started { track } => {
                app.decks.borrow_mut().stem_progress.insert(track, 0.0);
            }
            StemEvent::Progress { track, fraction } => {
                app.decks.borrow_mut().stem_progress.insert(track, fraction);
            }
            StemEvent::Done { track, .. } | StemEvent::AlreadyCached { track, .. } => {
                let rate = app.engine.borrow().sample_rate();
                let (tx, cache, decks) = {
                    let mut d = app.decks.borrow_mut();
                    d.stem_progress.remove(&track);
                    let decks: Vec<(usize, TrackSource, Beatgrid)> = (0..DECKS)
                        .filter_map(|i| d.loaded[i].as_ref().filter(|l| l.id == track).map(|l| (i, l.source.clone(), l.grid.clone())))
                        .collect();
                    (d.prepared_tx.clone(), d.stem_cache.clone(), decks)
                };
                for (deck, src, grid) in decks {
                    prepare_in_background(cache.clone(), tx.clone(), deck, track, src, rate, grid, None, Some(Swap::Stems));
                }
            }
            StemEvent::Failed { track, error } => {
                app.decks.borrow_mut().stem_progress.remove(&track);
                app.status(format!("Stem separation failed: {error}"));
            }
            StemEvent::Cancelled { track } => {
                app.decks.borrow_mut().stem_progress.remove(&track);
            }
        }
    }
}

#[cfg(not(feature = "separation"))]
pub(crate) fn poll_stems(_app: &App) {}

// ── View model ──────────────────────────────────────────────────────────

/// The scalar part of a deck view; compared frame to frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DeckFields {
    pub deck: i32,
    pub loaded: bool,
    pub title: SharedString,
    pub artist: SharedString,
    pub bpm: SharedString,
    pub key: SharedString,
    pub tempo: SharedString,
    pub elapsed: SharedString,
    pub remaining: SharedString,
    pub progress: f32,
    pub playing: bool,
    pub master: bool,
    pub sync: bool,
    pub keylock: bool,
    pub looping: bool,
    pub loop_beats: SharedString,
    pub slip: bool,
    pub level: f32,
    pub fader: f32,
    pub tempo_value: f32,
    pub tempo_range: SharedString,
    pub quantize: bool,
    pub stems_available: bool,
    pub stems_busy: bool,
    pub mute_vocal: bool,
    pub mute_inst: bool,
    pub mute_drums: bool,
    pub mute_bass: bool,
    pub loop_size: SharedString,
    pub phrase: SharedString,
    pub jog_angle: f32,
    pub beat_pos: SharedString,
}

/// What the projection needs besides the engine's deck state.
pub(crate) struct DeckInputs<'a> {
    pub deck: usize,
    pub meta: Option<&'a Loaded>,
    pub level: f32,
    pub fader: f32,
    pub quantize: bool,
    pub stems_busy: bool,
    /// Auto-loop size when no loop runs, in beats.
    pub loop_size: f64,
    /// Bar counter mode (Preferences › View).
    pub bar_counter: &'a str,
}

/// The deck's bar counter in the chosen mode (falls back to counting up).
fn bar_counter(s: &DeckState, meta: Option<&Loaded>, mode: &str) -> String {
    let up = || format::bar_beat(s.beat, s.beat_in_bar);
    let Some(m) = meta.filter(|_| s.beat_in_bar > 0) else { return up() };
    let beat = s.beat.max(0.0);
    let target = match mode {
        "to-phrase" => m.phrases.iter().find(|p| (p.start_beat as f64) <= beat && beat < p.end_beat as f64).map(|p| p.end_beat as f64),
        "to-memory-cue" => {
            let dur = s.duration.max(1e-6);
            m.cues
                .iter()
                .filter(|c| c.slot < 0)
                .map(|c| c.pos as f64 * dur)
                .filter(|&t| t > s.position + 0.01)
                .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.min(t))))
                .and_then(|t| m.grid.beat_index_at(t).map(|i| i as f64))
        }
        _ => None,
    };
    match target.map(|t| format::bar_countdown(beat, t)) {
        Some(c) if !c.is_empty() => c,
        _ => up(),
    }
}

pub(crate) fn tempo_range_label(r: TempoRange) -> &'static str {
    match r {
        TempoRange::Six => "±6%",
        TempoRange::Ten => "±10%",
        TempoRange::Sixteen => "±16%",
        TempoRange::Wide => "WIDE",
    }
}

/// "CHORUS -7.4" — the phrase at `beat` and the bars.beats left in it.
pub(crate) fn phrase_now(phrases: &[od_core::Phrase], beat: f64) -> String {
    let b = beat.max(0.0) as u32;
    phrases
        .iter()
        .find(|p| p.start_beat <= b && b < p.end_beat)
        .map(|p| format!("{} {}", render::phrase_label(p.kind), format::bar_countdown(beat, p.end_beat as f64)))
        .unwrap_or_default()
}

/// Projects one deck's engine state (pure).
pub(crate) fn deck_fields(s: &DeckState, i: &DeckInputs) -> DeckFields {
    let meta = i.meta;
    let muted = |stem: od_engine::Stem| s.stem_levels[stem.index()] < 0.5;
    DeckFields {
        deck: i.deck as i32 + 1,
        loaded: meta.is_some(),
        title: meta.map(|m| m.title.as_str()).unwrap_or_default().into(),
        artist: meta.map(|m| m.artist.as_str()).unwrap_or_default().into(),
        bpm: format::bpm(Some(s.bpm), 2).into(),
        key: meta.map(|m| m.key.as_str()).unwrap_or_default().into(),
        tempo: format::tempo(s.tempo).into(),
        elapsed: format::deck_time(s.position).into(),
        remaining: if s.duration > 0.0 { format!("-{}", format::deck_time(s.duration - s.position)) } else { String::new() }.into(),
        progress: if s.duration > 0.0 { (s.position / s.duration) as f32 } else { 0.0 },
        playing: s.playing,
        master: s.is_master,
        sync: s.sync,
        keylock: s.keylock,
        looping: s.loop_state.active,
        loop_beats: s.loop_state.beats.map(format::beats).unwrap_or_default().into(),
        slip: s.slip,
        level: i.level,
        fader: i.fader,
        tempo_value: s.tempo_fader,
        tempo_range: tempo_range_label(s.tempo_range).into(),
        quantize: i.quantize,
        stems_available: s.stems_available,
        stems_busy: i.stems_busy,
        mute_vocal: muted(od_engine::Stem::Vocals),
        mute_inst: muted(od_engine::Stem::Inst),
        mute_drums: muted(od_engine::Stem::Drums),
        mute_bass: muted(od_engine::Stem::Bass),
        loop_size: format::beats(s.loop_state.beats.filter(|_| s.loop_state.active).unwrap_or(i.loop_size)).into(),
        phrase: meta.map(|m| phrase_now(&m.phrases, s.beat)).unwrap_or_default().into(),
        // 33⅓ rpm: 200° per second of audio.
        jog_angle: (s.position * 200.0).rem_euclid(360.0) as f32,
        beat_pos: bar_counter(s, i.meta, i.bar_counter).into(),
    }
}

/// Pads A–H (or a column of them) from the engine's hot cues.
pub(crate) fn pad_slots(s: &DeckState, meta: Option<&Loaded>, slots: std::ops::Range<usize>) -> Vec<HotCueSlot> {
    slots
        .map(|slot| {
            let hc = s.hot_cues[slot];
            HotCueSlot {
                slot: slot as i32,
                letter: ((b'A' + slot as u8) as char).to_string().into(),
                set: hc.is_some(),
                time: hc.map(|h| format::deck_time(h.start)).unwrap_or_default().into(),
                label: meta.map(|m| m.labels[slot].clone()).unwrap_or_default().into(),
            }
        })
        .collect()
}

/// Identity of a rendered scrolling waveform: same key, same pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WaveKey {
    track: TrackId,
    /// Playhead in whole pixels.
    px: i64,
    /// Visible span in 0.1 ms.
    span: i64,
    width: u32,
    height: u32,
}

impl WaveKey {
    pub fn new(track: TrackId, position: f64, span: f64, width: u32, height: u32) -> Self {
        let px_per_sec = width as f64 / span.max(1e-6);
        Self { track, px: (position * px_per_sec).round() as i64, span: (span * 1e4).round() as i64, width, height }
    }
}

/// Cached view parts of one deck.
struct Lane {
    fields: Option<DeckFields>,
    /// Track the per-track parts below belong to.
    track: Option<TrackId>,
    cues: ModelRc<CueMark>,
    phrases: ModelRc<PhraseMark>,
    /// Phrases were shown when `phrases` was built (Preferences > View).
    phrases_on: bool,
    overview: Image,
    wave_key: Option<WaveKey>,
    waveform: Image,
    pads: Rows<HotCueSlot>,
    pads_left: Rows<HotCueSlot>,
    pads_right: Rows<HotCueSlot>,
}

impl Default for Lane {
    fn default() -> Self {
        Self {
            fields: None,
            track: None,
            cues: model(Vec::new()),
            phrases: model(Vec::new()),
            phrases_on: true,
            overview: Image::default(),
            wave_key: None,
            waveform: Image::default(),
            pads: Rows::default(),
            pads_left: Rows::default(),
            pads_right: Rows::default(),
        }
    }
}

impl Lane {
    fn view(&self) -> DeckView {
        let f = self.fields.clone().unwrap_or_default();
        DeckView {
            deck: f.deck,
            loaded: f.loaded,
            title: f.title,
            artist: f.artist,
            bpm: f.bpm,
            key: f.key,
            tempo: f.tempo,
            elapsed: f.elapsed,
            remaining: f.remaining,
            progress: f.progress,
            playing: f.playing,
            master: f.master,
            sync: f.sync,
            keylock: f.keylock,
            looping: f.looping,
            loop_beats: f.loop_beats,
            slip: f.slip,
            level: f.level,
            fader: f.fader,
            waveform: self.waveform.clone(),
            overview: self.overview.clone(),
            cues: self.cues.clone(),
            pads: self.pads.handle(),
            pads_left: self.pads_left.handle(),
            pads_right: self.pads_right.handle(),
            tempo_value: f.tempo_value,
            tempo_range: f.tempo_range,
            quantize: f.quantize,
            stems_available: f.stems_available,
            stems_busy: f.stems_busy,
            mute_vocal: f.mute_vocal,
            mute_inst: f.mute_inst,
            mute_drums: f.mute_drums,
            mute_bass: f.mute_bass,
            loop_size: f.loop_size,
            phrases: self.phrases.clone(),
            phrase: f.phrase,
            jog_angle: f.jog_angle,
            beat_pos: f.beat_pos,
        }
    }
}

/// The deck models bound to `decks`, `decks-left` (odd decks) and
/// `decks-right` (even decks).
#[derive(Default)]
pub(crate) struct DeckViews {
    pub all: Rows<DeckView>,
    pub left: Rows<DeckView>,
    pub right: Rows<DeckView>,
    lanes: [Lane; DECKS],
}

impl DeckViews {
    /// Re-creates the rows for `count` decks (empty plates).
    fn resize(&mut self, count: usize) {
        for (d, lane) in self.lanes.iter_mut().enumerate() {
            *lane = Lane::default();
            lane.fields = Some(DeckFields { deck: d as i32 + 1, ..Default::default() });
        }
        let views: Vec<DeckView> = (0..count).map(|d| self.lanes[d].view()).collect();
        let side = |odd: bool| views.iter().filter(|v| (v.deck % 2 == 1) == odd).cloned().collect::<Vec<_>>();
        self.left.sync(side(true), |_, _| false);
        self.right.sync(side(false), |_, _| false);
        self.all.sync(views, |_, _| false);
    }

    /// Writes deck `d` into the full list and its side column.
    fn publish(&self, d: usize) {
        let v = self.lanes[d].view();
        let side = if d % 2 == 0 { &self.left } else { &self.right };
        side.set(d / 2, v.clone());
        self.all.set(d, v);
    }
}

/// Before audio starts: empty plates so the Perform layout is complete.
pub(crate) fn show_empty(app: &App) {
    let mut d = app.decks.borrow_mut();
    let count = d.deck_count;
    if d.views.all.len() != count {
        d.views.resize(count);
    }
}

/// Frame tick: engine state → deck rows (only what changed).
pub(crate) fn project(app: &App, state: &EngineState) {
    let show_phrases = app.ui.get_show_phrases();
    let bar_mode = app.state.borrow().settings.view.bar_counter.clone();
    let scale = app.ui.window().scale_factor();
    let mut guard = app.decks.borrow_mut();
    let d = &mut *guard;
    let count = d.deck_count;
    let height = ((if count > 2 { 64.0 } else { 110.0 }) * scale) as u32;
    // Waveform area of a lane: the window minus the numeral gutter.
    let lane_width = ((app.ui.window().size().width as f32 / scale - 44.0).max(200.0) * scale) as u32;
    let now = Instant::now();
    if d.views.all.len() != count {
        d.views.resize(count);
    }
    for i in 0..count {
        let s = &state.decks[i];
        let meta = d.loaded[i].as_ref().filter(|m| Some(m.id) == s.track);
        let lane = &mut d.views.lanes[i];
        let mut dirty = false;
        let tid = meta.map(|m| m.id);
        if lane.track != tid || lane.fields.is_none() || lane.phrases_on != show_phrases {
            lane.track = tid;
            lane.phrases_on = show_phrases;
            lane.cues = model(meta.map(|m| m.cues.clone()).unwrap_or_default());
            lane.phrases = model(meta.filter(|_| show_phrases).map(|m| m.phrase_marks.clone()).unwrap_or_default());
            lane.overview = meta.map(|m| m.overview.clone()).unwrap_or_default();
            lane.waveform = Image::default();
            lane.wave_key = None;
            dirty = true;
        }
        // Zoomed waveform: tiles + motion (span follows tempo, so beats keep
        // their spacing; quantised to 10 ms to avoid re-rendering on every
        // tempo-fader step).
        let span = ((DETAIL_SPAN * d.zoom[i] * s.tempo.max(0.25)) * 100.0).round() / 100.0;
        let speed = if s.playing && !s.scratching { s.speed } else { 0.0 };
        let input = ScrollIn { meta, position: s.position, speed, span, width: lane_width, height, phrases: show_phrases, now };
        if let Some(row) = d.scroll[i].update(&input) {
            d.motion.set_row_data(i, row);
        }
        lane.pads.sync_eq(pad_slots(s, meta, 0..8));
        lane.pads_left.sync_eq(pad_slots(s, meta, 0..4));
        lane.pads_right.sync_eq(pad_slots(s, meta, 4..8));
        let fields = deck_fields(
            s,
            &DeckInputs {
                deck: i,
                meta,
                level: state.channels[i].meter.peak,
                fader: state.channels[i].fader,
                quantize: state.quantize,
                stems_busy: meta.is_some_and(|m| d.stem_progress.contains_key(&m.id)),
                loop_size: d.loop_size[i],
                bar_counter: &bar_mode,
            },
        );
        if lane.fields.as_ref() != Some(&fields) {
            lane.fields = Some(fields);
            dirty = true;
        }
        if dirty {
            d.views.publish(i);
        }
    }
}

// ── Controls ────────────────────────────────────────────────────────────

fn deck_state(app: &App, d: usize) -> Option<DeckState> {
    engine::snapshot(app).map(|s| s.decks[d])
}

/// New zoom after a scroll of `delta` logical pixels (up = closer), as in
/// rekordbox: from 1 s to 32 s across the lane.
pub(crate) fn zoom_by(zoom: f64, delta: f64) -> f64 {
    (zoom * (-delta * 0.004).exp()).clamp(0.125, 4.0)
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.set_lane_motion(ModelRc::from(app.decks.borrow().motion.clone()));
    ui.on_deck_zoom(handler!(app, |a, d: i32, delta: f32| {
        let i = (d as usize).saturating_sub(1).min(DECKS - 1);
        let mut decks = a.decks.borrow_mut();
        decks.zoom[i] = zoom_by(decks.zoom[i], delta as f64);
    }));
    ui.on_deck_zoom_reset(handler!(app, |a, d: i32| {
        let i = (d as usize).saturating_sub(1).min(DECKS - 1);
        a.decks.borrow_mut().zoom[i] = 1.0;
    }));
    // Deck callbacks carry 1-based deck numbers.
    let idx = |d: i32| (d as usize).saturating_sub(1).min(DECKS - 1);
    ui.on_deck_toggle_play(handler!(app, |a, d: i32| engine::deck_cmd(&a, d as usize - 1, DeckCommand::TogglePlay)));
    ui.on_deck_cue(handler!(app, |a, d: i32| {
        engine::deck_cmd(&a, d as usize - 1, DeckCommand::Cue(true));
        engine::deck_cmd(&a, d as usize - 1, DeckCommand::Cue(false));
    }));
    ui.on_deck_sync(handler!(app, |a, d: i32| {
        let on = deck_state(&a, d as usize - 1).map(|s| s.sync).unwrap_or(false);
        engine::deck_cmd(&a, d as usize - 1, DeckCommand::SetSync(!on));
    }));
    ui.on_deck_seek(handler!(app, |a, d: i32, f: f32| engine::deck_cmd(&a, d as usize - 1, DeckCommand::NeedleSearch { fraction: f as f64 })));
    ui.on_deck_hot_cue(handler!(app, |a, d: i32, s: i32| {
        engine::deck_cmd(&a, d as usize - 1, DeckCommand::HotCue { slot: s as usize, pressed: true });
        engine::deck_cmd(&a, d as usize - 1, DeckCommand::HotCue { slot: s as usize, pressed: false });
    }));
    ui.on_set_deck_count(handler!(app, |a, n: i32| a.decks.borrow_mut().deck_count = (n as usize).clamp(2, 4)));

    // These start audio on first use.
    ui.on_deck_master(handler!(app, |a, d: i32| {
        engine::ensure_engine(&a);
        engine::send(&a, Command::SetMasterDeck(Some(idx(d))));
    }));
    ui.on_deck_keylock(handler!(app, |a, d: i32| {
        engine::ensure_engine(&a);
        let d = idx(d);
        let on = deck_state(&a, d).map(|s| s.keylock).unwrap_or(false);
        engine::deck_cmd(&a, d, DeckCommand::SetKeylock(!on));
    }));
    ui.on_deck_quantize(handler!(app, |a, _d: i32| {
        engine::ensure_engine(&a);
        let q = engine::snapshot(&a).map(|s| s.quantize).unwrap_or(true);
        engine::send(&a, Command::SetQuantize(!q));
    }));
    ui.on_deck_slip(handler!(app, |a, d: i32| {
        engine::ensure_engine(&a);
        let d = idx(d);
        let on = deck_state(&a, d).map(|s| s.slip).unwrap_or(false);
        engine::deck_cmd(&a, d, DeckCommand::SetSlip(!on));
    }));
    ui.on_deck_tempo(handler!(app, |a, d: i32, v: f32| {
        engine::ensure_engine(&a);
        engine::deck_cmd(&a, idx(d), DeckCommand::SetTempoFader(v));
    }));
    ui.on_deck_stem(handler!(app, |a, d: i32, stem: SharedString| {
        engine::ensure_engine(&a);
        let d = idx(d);
        let s = match stem.as_str() {
            "vocal" => od_engine::Stem::Vocals,
            "drums" => od_engine::Stem::Drums,
            "bass" => od_engine::Stem::Bass,
            _ => od_engine::Stem::Inst,
        };
        let level = deck_state(&a, d).map(|st| st.stem_levels[s.index()]).unwrap_or(1.0);
        engine::deck_cmd(&a, d, DeckCommand::SetStemLevel { stem: s, level: if level < 0.5 { 1.0 } else { 0.0 } });
    }));
    ui.on_deck_loop(handler!(app, |a, d: i32, beats: f32| {
        engine::ensure_engine(&a);
        let d = idx(d);
        let b = if beats > 0.0 { beats as f64 } else { a.decks.borrow().loop_size[d] };
        engine::deck_cmd(&a, d, DeckCommand::AutoLoop { beats: b });
    }));
    ui.on_deck_loop_exit(handler!(app, |a, d: i32| {
        engine::ensure_engine(&a);
        engine::deck_cmd(&a, idx(d), DeckCommand::LoopExit);
    }));
    ui.on_deck_loop_scale(handler!(app, |a, d: i32, f: f32| {
        engine::ensure_engine(&a);
        let d = idx(d);
        let looping = deck_state(&a, d).map(|s| s.loop_state.active).unwrap_or(false);
        if looping {
            engine::deck_cmd(&a, d, if f < 1.0 { DeckCommand::LoopHalve } else { DeckCommand::LoopDouble });
        }
        let mut decks = a.decks.borrow_mut();
        decks.loop_size[d] = (decks.loop_size[d] * f as f64).clamp(1.0 / 32.0, 32.0);
    }));
    ui.on_deck_beat_jump(handler!(app, |a, d: i32, beats: f32| {
        engine::ensure_engine(&a);
        engine::deck_cmd(&a, idx(d), DeckCommand::BeatJump { beats: beats as f64 });
    }));
    ui.on_deck_load(handler!(app, |a, d: i32| {
        engine::ensure_engine(&a);
        load_selected(&a, idx(d));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoload_spec() {
        assert_eq!(parse_autoload("1:12, 2:13"), vec![(1, TrackId(12)), (2, TrackId(13))]);
        assert_eq!(parse_autoload("5:1,x:2,3:y,4"), vec![]);
        assert_eq!(parse_autoload(""), vec![]);
    }

    #[test]
    fn free_deck_prefers_idle_empty() {
        assert_eq!(free_deck(2, &[false, false], |d| d == 0), 1);
        assert_eq!(free_deck(2, &[true, false], |_| true), 1);
        assert_eq!(free_deck(2, &[true, true], |_| true), 0);
        assert_eq!(free_deck(4, &[false; 4], |_| false), 0);
    }

    #[test]
    fn empty_deck_projection() {
        let s = DeckState::default();
        let f = deck_fields(&s, &DeckInputs { deck: 1, meta: None, level: 0.0, fader: 1.0, quantize: true, stems_busy: false, loop_size: 0.5, bar_counter: "up" });
        assert_eq!(f.deck, 2);
        assert!(!f.loaded);
        assert_eq!(f.bpm, "");
        assert_eq!(f.remaining, "");
        assert_eq!(f.loop_size, "1/2");
        assert_eq!(f.elapsed, "0:00.0");
        // Unchanged state projects equal, so no row is written.
        let g = deck_fields(&s, &DeckInputs { deck: 1, meta: None, level: 0.0, fader: 1.0, quantize: true, stems_busy: false, loop_size: 0.5, bar_counter: "up" });
        assert_eq!(f, g);
    }

    #[test]
    fn waveform_key_changes_per_pixel() {
        let t = TrackId(1);
        // 800 px over 8 s: 100 px per second.
        let a = WaveKey::new(t, 10.000, 8.0, 800, 100);
        assert_eq!(a, WaveKey::new(t, 10.004, 8.0, 800, 100));
        assert_ne!(a, WaveKey::new(t, 10.01, 8.0, 800, 100));
        assert_ne!(a, WaveKey::new(t, 10.0, 8.08, 800, 100));
        assert_ne!(a, WaveKey::new(t, 10.0, 8.0, 801, 100));
    }

    #[test]
    fn tempo_ranges() {
        assert_eq!(tempo_range_label(TempoRange::Ten), "±10%");
        assert_eq!(tempo_range_label(TempoRange::Wide), "WIDE");
    }
}
