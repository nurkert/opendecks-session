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
use crate::{CueMark, DeckView, LaneMotion, PadView, PhraseMark, engine, format, pads, player, render};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_controller::PadMode;
use od_core::{Beatgrid, CueKind, TrackId, Waveform};
use od_engine::{Command, DeckCommand, DeckState, EngineState, TempoRange};
use slint::{ComponentHandle, Image, Model, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

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
    /// Cover art thumbnail.
    artwork: Option<Image>,
    pub title: String,
    pub artist: String,
    key: String,
    /// The track's key for key sync.
    musical_key: Option<od_core::MusicalKey>,
    waveform: Waveform,
    grid: Beatgrid,
    overview: Image,
    cues: Vec<CueMark>,
    /// Hot cue labels by library index (bank * 8 + slot).
    labels: Vec<String>,
    phrases: Vec<od_core::Phrase>,
    phrase_marks: Vec<PhraseMark>,
    /// What a reload with stems needs.
    source: TrackSource,
}

/// "SPLIT 42%" while parts are separated (fraction 0..1), "SPLIT…" at the start.
pub(crate) fn split_text(progress: Option<f32>) -> String {
    match progress {
        Some(p) if p > 0.005 => format!("SPLIT {:.0}%", (p * 100.0).clamp(0.0, 99.0)),
        Some(_) => "SPLIT…".into(),
        None => String::new(),
    }
}

/// The stem cache a stick carries (`/OPENDECKS/stems`).
pub(crate) fn stick_stem_cache(root: &std::path::Path) -> od_stems::StemCache {
    od_stems::StemCache::new(root.join("OPENDECKS").join("stems"))
}

impl Loaded {
    /// Memory cue starts in seconds, in track order.
    pub(crate) fn memory_starts(&self, duration: f64) -> Vec<f64> {
        let mut v: Vec<f64> = self.cues.iter().filter(|c| c.slot < 0).map(|c| c.pos as f64 * duration).collect();
        v.sort_by(f64::total_cmp);
        v
    }
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
    /// Hot cue the Keyboard pads play, per deck (0..8).
    pub keyboard_cue: [usize; DECKS],
    /// Slicer domain per deck, in beats.
    pub slicer_beats: [f64; DECKS],
    /// TAP BPM per deck (controller).
    taps: [crate::fx::TapTempo; DECKS],
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
            keyboard_cue: [0; DECKS],
            slicer_beats: [8.0; DECKS],
            taps: Default::default(),
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
    key: Option<(TrackId, i64, u32, u32, bool, bool)>,
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
        let key = (m.id, (i.span * 1000.0).round() as i64, i.width, i.height, i.phrases, crate::appearance::is_light());
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
    let artwork = st.src().artwork(track).ok().flatten().and_then(|(data, _)| render::artwork_thumbnail(&data, (72.0 * scale) as u32));
    let meta = Loaded {
        id: track,
        artwork,
        title: t.tags.title.clone(),
        artist: t.tags.artist.clone(),
        key: st.key_text(&t),
        musical_key: t.key,
        overview: render::overview(&analysis.waveform, &analysis.beatgrid, t.duration, (900.0 * scale) as u32, (30.0 * scale) as u32),
        waveform: analysis.waveform.clone(),
        grid: analysis.beatgrid.clone(),
        cues: cue_marks(&cues, t.duration, false),
        labels: hot_labels(&cues),
        phrases: analysis.phrases.clone(),
        phrase_marks: phrase_marks(&analysis.phrases, &analysis.beatgrid, t.duration),
        source: source.clone(),
    };
    let read_only = st.read_only();
    drop(st);
    app.ui.set_engine_message(format!("Loading {} …", meta.title).into());
    // Tracks from a stick use the parts separated on the desktop and carried
    // onto the stick (SPEC §10.4), so a Raspberry Pi plays them too.
    let stick_stems = app.state.borrow().stick_root().map(stick_stem_cache);
    let (cache, tx) = {
        let mut d = app.decks.borrow_mut();
        d.pending[deck] = Some(meta);
        d.loaded_from_stick = read_only;
        (stick_stems.unwrap_or_else(|| d.stem_cache.clone()), d.prepared_tx.clone())
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
        let musical_key = meta.musical_key;
        if needs_stems && deck != PREVIEW_DECK {
            request_stems(app, meta.id, &meta.source);
        }
        app.decks.borrow_mut().loaded[deck] = Some(meta);
        crate::browser::refresh_tracks(app);
        engine::deck_cmd(app, deck, DeckCommand::Load(pr.data));
        engine::deck_cmd(app, deck, DeckCommand::SetTrackKey(musical_key));
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
            m.musical_key = Some(k);
        }
        m.source.gain = a.loudness_lufs.map(|l| (TARGET_LUFS - l).clamp(-12.0, 12.0)).unwrap_or(0.0);
        swaps.push((i, m.id, m.source.clone(), m.grid.clone(), m.musical_key));
        // Rebuild the per-track view parts on the next frame.
        d.views.lanes[i].track = None;
    }
    let (cache, tx) = (d.stem_cache.clone(), d.prepared_tx.clone());
    drop(d);
    drop(st);
    for (deck, id, src, grid, key) in swaps {
        engine::deck_cmd(app, deck, DeckCommand::SetTrackKey(key));
        prepare_in_background(cache.clone(), tx.clone(), deck, id, src, rate, grid, None, Some(Swap::Analysis));
    }
}

// ── Stems ───────────────────────────────────────────────────────────────

#[cfg(test)]
impl Decks {
    /// A separation job was queued (the worker exists).
    pub(crate) fn stems_requested(&self) -> bool {
        self.stem_worker.is_some()
    }
}

/// Queues stem separation of a loaded track (desktop profile only).
#[cfg(feature = "separation")]
fn request_stems(app: &App, track: TrackId, src: &TrackSource) {
    if app.state.borrow().config.lite {
        return;
    }
    // Headless runs (UI tests) never download the model.
    let fetch_model = !app.state.borrow().config.headless;
    let mut d = app.decks.borrow_mut();
    if d.stem_progress.contains_key(&track) {
        return;
    }
    let cache = d.stem_cache.clone();
    let worker = d.stem_worker.get_or_insert_with(|| od_stems::StemWorker::spawn(cache, od_stems::StemWorkerOptions { fetch_model, ..Default::default() }));
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
    pub split_text: SharedString,
    pub mute_vocal: bool,
    pub mute_inst: bool,
    pub mute_drums: bool,
    pub mute_bass: bool,
    pub level_vocal: f32,
    pub level_inst: f32,
    pub level_drums: f32,
    pub level_bass: f32,
    pub loop_size: SharedString,
    pub phrase: SharedString,
    pub jog_angle: f32,
    pub beat_pos: SharedString,
    pub pad_mode: SharedString,
    pub pad_param: SharedString,
    pub key_shift: SharedString,
    pub key_sync: bool,
    pub jump_size: SharedString,
}

/// What the projection needs besides the engine's deck state.
pub(crate) struct DeckInputs<'a> {
    pub deck: usize,
    pub meta: Option<&'a Loaded>,
    pub level: f32,
    pub fader: f32,
    pub quantize: bool,
    /// Separation progress (0..1) while parts are being separated.
    pub stems_busy: Option<f32>,
    /// Auto-loop size when no loop runs, in beats.
    pub loop_size: f64,
    /// Bar counter mode (Preferences › View).
    pub bar_counter: &'a str,
    pub pad_mode: PadMode,
    /// Value of the pad mode's ‹ › (see [`pads::param_label`]).
    pub pad_param: String,
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
        stems_busy: i.stems_busy.is_some(),
        split_text: split_text(i.stems_busy).into(),
        mute_vocal: muted(od_engine::Stem::Vocals),
        mute_inst: muted(od_engine::Stem::Inst),
        mute_drums: muted(od_engine::Stem::Drums),
        mute_bass: muted(od_engine::Stem::Bass),
        level_vocal: s.stem_levels[od_engine::Stem::Vocals.index()],
        level_inst: s.stem_levels[od_engine::Stem::Inst.index()],
        level_drums: s.stem_levels[od_engine::Stem::Drums.index()],
        level_bass: s.stem_levels[od_engine::Stem::Bass.index()],
        loop_size: format::beats(s.loop_state.beats.filter(|_| s.loop_state.active).unwrap_or(i.loop_size)).into(),
        phrase: meta.map(|m| phrase_now(&m.phrases, s.beat)).unwrap_or_default().into(),
        // 33⅓ rpm: 200° per second of audio.
        jog_angle: (s.position * 200.0).rem_euclid(360.0) as f32,
        beat_pos: bar_counter(s, i.meta, i.bar_counter).into(),
        pad_mode: i.pad_mode.name().into(),
        pad_param: i.pad_param.as_str().into(),
        key_shift: pads::key_shift_label(s.key_shift).into(),
        key_sync: s.key_sync,
        jump_size: pads::beats_label(if s.beat_jump_size > 0.0 { s.beat_jump_size } else { 4.0 }).into(),
    }
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
    #[cfg(test)]
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
    artwork: Option<Image>,
    wave_key: Option<WaveKey>,
    waveform: Image,
    pads: Rows<PadView>,
    pads_left: Rows<PadView>,
    pads_right: Rows<PadView>,
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
            artwork: None,
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
            artwork: self.artwork.clone().unwrap_or_default(),
            has_artwork: self.artwork.is_some(),
            cues: self.cues.clone(),
            pads: self.pads.handle(),
            pads_left: self.pads_left.handle(),
            pads_right: self.pads_right.handle(),
            tempo_value: f.tempo_value,
            tempo_range: f.tempo_range,
            quantize: f.quantize,
            stems_available: f.stems_available,
            stems_busy: f.stems_busy,
            split_text: f.split_text,
            mute_vocal: f.mute_vocal,
            mute_inst: f.mute_inst,
            mute_drums: f.mute_drums,
            mute_bass: f.mute_bass,
            level_vocal: f.level_vocal,
            level_inst: f.level_inst,
            level_drums: f.level_drums,
            level_bass: f.level_bass,
            loop_size: f.loop_size,
            phrases: self.phrases.clone(),
            phrase: f.phrase,
            jog_angle: f.jog_angle,
            beat_pos: f.beat_pos,
            pad_mode: f.pad_mode,
            pad_param: f.pad_param,
            key_shift: f.key_shift,
            key_sync: f.key_sync,
            jump_size: f.jump_size,
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
            // Empty decks show their hot cue pads and mode too.
            let s = DeckState::default();
            let ctx = pads::PadCtx { deck: d, mode: PadMode::HotCue, sampler_bank: 0, keyboard_cue: 0, slicer_beats: 8.0, labels: &[], sampler_slots: &[], memory: &[] };
            let rows = pads::pad_views(&s, &Default::default(), &[], &ctx);
            lane.pads.sync_eq(rows.clone());
            lane.pads_left.sync_eq(rows[..4].to_vec());
            lane.pads_right.sync_eq(rows[4..].to_vec());
            lane.fields = Some(DeckFields {
                deck: d as i32 + 1,
                pad_mode: PadMode::HotCue.name().into(),
                pad_param: pads::param_label(&s, &ctx).into(),
                key_shift: pads::key_shift_label(0.0).into(),
                jump_size: "4".into(),
                ..Default::default()
            });
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
    let sampler_bank = app.sampler.bank.get();
    let sampler_slots = app.sampler.slots.borrow();
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
            lane.artwork = meta.and_then(|m| m.artwork.clone());
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
        let memory = meta.map(|m| m.memory_starts(s.duration)).unwrap_or_default();
        let ctx = pads::PadCtx {
            deck: i,
            mode: d.pad_mode[i],
            sampler_bank,
            keyboard_cue: d.keyboard_cue[i],
            slicer_beats: d.slicer_beats[i],
            labels: meta.map(|m| m.labels.as_slice()).unwrap_or(&[]),
            sampler_slots: &sampler_slots,
            memory: &memory,
        };
        let mut rows = pads::pad_views(s, &state.channels[i], &state.sampler, &ctx);
        if d.pad_mode[i] == PadMode::Lighting {
            crate::lighting::light_pads(app, &mut rows);
        }
        if d.pad_mode[i] == PadMode::HotCue && let Some(m) = meta {
            // Hot cue pads in their stored colours.
            let bank = s.hot_cue_bank * od_engine::HOT_CUES;
            for r in rows.iter_mut() {
                if let Some(c) = m.cues.iter().find(|c| c.slot == bank as i32 + r.index) {
                    r.tone = c.tone;
                }
            }
        }
        let pad_param = pads::param_label(s, &ctx);
        lane.pads.sync_eq(rows.clone());
        lane.pads_left.sync_eq(rows[..4].to_vec());
        lane.pads_right.sync_eq(rows[4..].to_vec());
        let fields = deck_fields(
            s,
            &DeckInputs {
                deck: i,
                meta,
                level: state.channels[i].meter.peak,
                fader: state.channels[i].fader,
                quantize: state.quantize,
                stems_busy: meta.and_then(|m| d.stem_progress.get(&m.id).copied()),
                loop_size: d.loop_size[i],
                bar_counter: &bar_mode,
                pad_mode: d.pad_mode[i],
                pad_param,
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

/// Hot cues set or deleted while performing (pads, controller) become
/// library edits: one undo step each, synced to sticks like any edit. The
/// cue keeps its label and colour when only its position changes.
pub(crate) fn persist_cue_events(app: &App, events: &[od_engine::Event]) {
    use od_engine::Event;
    // Tracks played from a stick are read-only here.
    if events.is_empty() || app.state.borrow().read_only() || app.decks.borrow().loaded_from_stick {
        return;
    }
    for ev in events {
        let (track, index, cue) = match *ev {
            Event::HotCueSet { deck, track, bank, slot, cue } if deck != PREVIEW_DECK => (track, bank * 8 + slot, Some(cue)),
            Event::HotCueDeleted { deck, track, bank, slot } if deck != PREVIEW_DECK => (track, bank * 8 + slot, None),
            _ => continue,
        };
        let Ok(index) = u8::try_from(index) else { continue };
        let current = app.state.borrow().lib.cues(track).unwrap_or_default();
        let next = apply_hot_cue(current.clone(), index, cue);
        if next == current {
            continue;
        }
        let result = app.state.borrow_mut().lib.set_cues(track, next);
        match result {
            Ok(()) => {
                refresh_deck_cues(app, track);
                app.after_edit("");
            }
            Err(e) => tracing::warn!(error = %e, "could not save a hot cue set while performing"),
        }
    }
}

/// Rebuilds the cue marks and pad labels of every deck showing `track`.
fn refresh_deck_cues(app: &App, track: TrackId) {
    let cues = app.state.borrow().lib.cues(track).unwrap_or_default();
    let duration = app.state.borrow().lib.track(track).map(|t| t.duration).unwrap_or(0.0);
    let mut d = app.decks.borrow_mut();
    for i in 0..DECKS {
        if let Some(m) = d.loaded[i].as_mut().filter(|m| m.id == track) {
            m.cues = cue_marks(&cues, duration, false);
            m.labels = hot_labels(&cues);
            // Force the lane to rebuild its per-track models.
            d.views.lanes[i].track = None;
        }
    }
}

/// Hot cue labels by library index, for every bank.
fn hot_labels(cues: &[od_core::Cue]) -> Vec<String> {
    (0..od_engine::HOT_CUE_BANKS * od_engine::HOT_CUES).map(|s| cues.iter().find(|c| c.kind == CueKind::Hot(s as u8)).map(|c| c.label.clone()).unwrap_or_default()).collect()
}

/// The cue list with hot cue `index` set to `cue` (or removed when `None`).
pub(crate) fn apply_hot_cue(mut cues: Vec<od_core::Cue>, index: u8, cue: Option<od_engine::HotCue>) -> Vec<od_core::Cue> {
    let pos = cues.iter().position(|c| c.kind == CueKind::Hot(index));
    match (cue, pos) {
        (Some(h), Some(i)) => {
            cues[i].start = h.start;
            cues[i].loop_end = h.loop_end;
        }
        (Some(h), None) => {
            let mut c = od_core::Cue::hot(index, h.start);
            c.loop_end = h.loop_end;
            cues.push(c);
        }
        (None, Some(i)) => {
            cues.remove(i);
        }
        (None, None) => {}
    }
    cues
}

/// New zoom after a scroll of `delta` logical pixels (up = closer): from
/// 1 s to 32 s across the lane.
pub(crate) fn zoom_by(zoom: f64, delta: f64) -> f64 {
    (zoom * (-delta * 0.004).exp()).clamp(0.125, 4.0)
}

// ── Pad modes, key shift ────────────────────────────────────────────────

/// Runs `f` with the pad context of deck `d` and its engine state (starts
/// audio first).
fn with_pads<R>(app: &App, d: usize, f: impl FnOnce(&DeckState, &pads::PadCtx) -> R) -> R {
    engine::ensure_engine(app);
    let s = deck_state(app, d).unwrap_or_default();
    let slots = app.sampler.slots.borrow();
    let dk = app.decks.borrow();
    let labels = dk.loaded(d).map(|m| m.labels.clone()).unwrap_or_default();
    let memory = dk.loaded(d).map(|m| m.memory_starts(s.duration)).unwrap_or_default();
    let ctx = pads::PadCtx {
        deck: d,
        mode: dk.pad_mode[d],
        sampler_bank: app.sampler.bank.get(),
        keyboard_cue: dk.keyboard_cue[d],
        slicer_beats: dk.slicer_beats[d],
        labels: &labels,
        sampler_slots: &slots,
        memory: &memory,
    };
    f(&s, &ctx)
}

/// The pads of deck `d` in its mode (for the controller's LEDs).
pub(crate) fn pad_rows(app: &App, d: usize, state: &EngineState) -> Vec<PadView> {
    let slots = app.sampler.slots.borrow();
    let dk = app.decks.borrow();
    let labels = dk.loaded(d).map(|m| m.labels.as_slice()).unwrap_or(&[]);
    let memory = dk.loaded(d).map(|m| m.memory_starts(state.decks[d].duration)).unwrap_or_default();
    let ctx = pads::PadCtx {
        deck: d,
        mode: dk.pad_mode[d],
        sampler_bank: app.sampler.bank.get(),
        keyboard_cue: dk.keyboard_cue[d],
        slicer_beats: dk.slicer_beats[d],
        labels,
        sampler_slots: &slots,
        memory: &memory,
    };
    pads::pad_views(&state.decks[d], &state.channels[d], &state.sampler, &ctx)
}

/// Pad `index` (0..8) of deck `d` pressed or released, in the deck's mode.
pub(crate) fn pad(app: &App, d: usize, index: usize, pressed: bool) {
    if app.decks.borrow().pad_mode[d] == PadMode::Lighting {
        crate::lighting::pad(app, index, pressed);
        return;
    }
    let cmds = with_pads(app, d, |s, ctx| pads::press_commands(s, ctx, index, pressed));
    for c in cmds {
        engine::send(app, c);
    }
}

/// The shifted pad (right click): deletes a hot cue, stops a sample.
pub(crate) fn pad_alt(app: &App, d: usize, index: usize) {
    let mode = app.decks.borrow().pad_mode[d];
    engine::ensure_engine(app);
    match mode {
        PadMode::HotCue => engine::deck_cmd(app, d, DeckCommand::DeleteHotCue { slot: index.min(7) }),
        PadMode::Sampler => {
            let slot = pads::sampler_slot(d, app.sampler.bank.get(), index);
            engine::send(app, Command::Sampler(od_engine::SamplerCommand::Stop { slot }));
        }
        _ => {}
    }
}

/// Switches deck `d` to pad mode `mode`.
pub(crate) fn set_pad_mode(app: &App, d: usize, mode: PadMode) {
    let (old, beats) = {
        let dk = app.decks.borrow();
        (dk.pad_mode[d], dk.slicer_beats[d])
    };
    if old != mode {
        engine::ensure_engine(app);
        for c in pads::mode_change_commands(d, old, mode, beats) {
            engine::send(app, c);
        }
    }
    app.decks.borrow_mut().pad_mode[d] = mode;
}

/// The ‹ › beside the pad mode: bank, page, jump size, key, slice length.
pub(crate) fn pad_param(app: &App, d: usize, delta: i32) {
    engine::ensure_engine(app);
    let s = deck_state(app, d).unwrap_or_default();
    let mode = app.decks.borrow().pad_mode[d];
    match mode {
        PadMode::HotCue => engine::deck_cmd(app, d, DeckCommand::SetHotCueBank(pads::step_hot_cue_bank(s.hot_cue_bank, delta))),
        PadMode::PadFx1 => set_pad_mode(app, d, PadMode::PadFx2),
        PadMode::PadFx2 => set_pad_mode(app, d, PadMode::PadFx1),
        PadMode::BeatJump => step_jump_size(app, d, delta),
        PadMode::Sampler => crate::sampler::step_bank_by(app, delta),
        PadMode::Keyboard => {
            let mut dk = app.decks.borrow_mut();
            dk.keyboard_cue[d] = (dk.keyboard_cue[d] as i32 + delta).rem_euclid(8) as usize;
        }
        PadMode::KeyShift => key_shift(app, d, delta),
        PadMode::Slicer => {
            let beats = pads::step_in(&pads::SLICER_BEATS, app.decks.borrow().slicer_beats[d], delta);
            app.decks.borrow_mut().slicer_beats[d] = beats;
            if s.slicer.active {
                engine::deck_cmd(app, d, DeckCommand::Slicer(od_engine::SlicerCommand::Enable { beats, loop_mode: s.slicer.loop_mode }));
            }
        }
        PadMode::BeatLoop | PadMode::Stems | PadMode::MemoryCue | PadMode::Lighting => {}
    }
}

/// Beat jump size one step up or down.
pub(crate) fn step_jump_size(app: &App, d: usize, delta: i32) {
    let cur = deck_state(app, d).map(|s| s.beat_jump_size).filter(|b| *b > 0.0).unwrap_or(4.0);
    engine::deck_cmd(app, d, DeckCommand::SetBeatJumpSize(pads::step_in(&pads::JUMP_SIZES, cur, delta)));
}

/// Key shift by `delta` semitones (0 resets), −12 … +12.
pub(crate) fn key_shift(app: &App, d: usize, delta: i32) {
    engine::ensure_engine(app);
    let cur = deck_state(app, d).map(|s| s.key_shift).unwrap_or(0.0);
    let next = if delta == 0 { 0.0 } else { (cur.round() + delta as f32).clamp(-12.0, 12.0) };
    engine::deck_cmd(app, d, DeckCommand::SetKeyShift(next));
}

pub(crate) fn toggle_key_sync(app: &App, d: usize) {
    engine::ensure_engine(app);
    let on = deck_state(app, d).map(|s| s.key_sync).unwrap_or(false);
    engine::deck_cmd(app, d, DeckCommand::SetKeySync(!on));
}

// ── Cue and grid edits from the controller ──────────────────────────────

/// A constant grid at `bpm` that keeps the first downbeat of `grid` (pure).
pub(crate) fn tapped_grid(grid: &Beatgrid, bpm: f64, duration: f64) -> Beatgrid {
    let anchor = grid.beats.iter().find(|b| b.bar_position == 1).or(grid.beats.first()).map(|b| b.time).unwrap_or(0.0);
    let mut g = Beatgrid::constant(bpm, anchor, 1, duration.max(anchor + 1.0));
    g.locked = grid.locked;
    g
}

/// The cue list with a memory cue added at `pos` (none within 50 ms), or
/// the memory cue nearest to `pos` (within 1 s) removed (pure).
pub(crate) fn edit_memory_cue(mut cues: Vec<od_core::Cue>, pos: f64, add: bool) -> Vec<od_core::Cue> {
    let memory = |c: &od_core::Cue| c.kind == CueKind::Memory;
    if add {
        if !cues.iter().any(|c| memory(c) && (c.start - pos).abs() < 0.05) {
            cues.push(od_core::Cue::memory(pos));
        }
    } else if let Some(i) = cues
        .iter()
        .enumerate()
        .filter(|(_, c)| memory(c) && (c.start - pos).abs() <= 1.0)
        .min_by(|a, b| (a.1.start - pos).abs().total_cmp(&(b.1.start - pos).abs()))
        .map(|(i, _)| i)
    {
        cues.remove(i);
    }
    cues
}

/// Makes the memory cue at `start` a saved loop ending at `end`.
pub(crate) fn set_memory_loop_end(cues: &mut [od_core::Cue], start: f64, end: f64) {
    if let Some(c) = cues.iter_mut().find(|c| c.kind == CueKind::Memory && (c.start - start).abs() < 0.05) {
        c.loop_end = Some(end);
    }
}

/// Gives the engine a fresh build of deck `d`'s track (new cues or grid)
/// without a gap.
fn reload_engine_data(app: &App, d: usize) {
    let rate = app.engine.borrow().sample_rate();
    let dk = app.decks.borrow();
    let Some(m) = dk.loaded(d) else { return };
    let (cache, tx, id, src, grid) = (dk.stem_cache.clone(), dk.prepared_tx.clone(), m.id, m.source.clone(), m.grid.clone());
    drop(dk);
    prepare_in_background(cache, tx, d, id, src, rate, grid, None, Some(Swap::Analysis));
}

/// Whether deck `d`'s track may be edited (not from a stick).
fn editable(app: &App, d: usize) -> Option<TrackId> {
    let dk = app.decks.borrow();
    if dk.loaded_from_stick {
        app.status("Tracks on a stick are read-only here — edit them in your library and the stick follows");
        return None;
    }
    dk.loaded_id(d)
}

/// MEMORY (controller): stores a memory cue at the playhead, or deletes the
/// one at the playhead; saved to the library like any cue edit.
pub(crate) fn memory_cue(app: &App, d: usize, add: bool) {
    let Some(id) = editable(app, d) else { return };
    let Some(st) = deck_state(app, d) else { return };
    // While a loop plays, MEMORY stores the loop (a saved memory loop).
    let looped = st.loop_state.start.zip(st.loop_state.end).filter(|_| st.loop_state.active);
    let pos = looped.map_or(st.position, |(a, _)| a);
    let current = app.state.borrow().lib.cues(id).unwrap_or_default();
    let mut next = edit_memory_cue(current.clone(), pos, add);
    if add && let Some((_, end)) = looped {
        set_memory_loop_end(&mut next, pos, end);
    }
    if next == current {
        return;
    }
    let r = app.state.borrow_mut().lib.set_cues(id, next.clone());
    if app.report(r).is_none() {
        return;
    }
    if let Some(m) = app.decks.borrow_mut().loaded[d].as_mut() {
        m.source.cues = next;
    }
    refresh_deck_cues(app, id);
    reload_engine_data(app, d);
    app.after_edit(if add { "Memory cue added" } else { "Memory cue deleted" });
}

/// TAP (controller): after three taps the track gets a constant grid at the
/// tapped tempo, keeping its first downbeat.
pub(crate) fn tap_bpm(app: &App, d: usize) {
    let t = TAP_EPOCH.with(|e| e.elapsed().as_secs_f64());
    let Some(bpm) = app.decks.borrow_mut().taps[d].tap(t) else { return };
    let Some(id) = editable(app, d) else { return };
    let bpm = (bpm as f64 * 100.0).round() / 100.0;
    let grid = app.state.borrow().lib.beatgrid(id).ok().flatten().unwrap_or_default();
    if grid.locked {
        app.status("The beatgrid is locked — unlock it in Prepare to tap the tempo");
        return;
    }
    let duration = deck_state(app, d).map(|s| s.duration).unwrap_or(0.0);
    let next = tapped_grid(&grid, bpm, duration);
    let r = app.state.borrow_mut().lib.set_beatgrid(id, next.clone());
    if app.report(r).is_none() {
        return;
    }
    {
        let mut dk = app.decks.borrow_mut();
        if let Some(m) = dk.loaded[d].as_mut() {
            m.grid = next;
        }
        dk.scroll[d] = LaneScroll::default();
        dk.views.lanes[d].track = None;
    }
    reload_engine_data(app, d);
    app.after_edit(&format!("Tapped tempo: {bpm:.2} BPM"));
}

thread_local! {
    static TAP_EPOCH: Instant = Instant::now();
}

/// A part by its UI name ("vocal", "inst", "drums", "bass").
pub(crate) fn stem_of_ui(name: &str) -> Option<od_engine::Stem> {
    match name {
        "vocal" | "vocals" => Some(od_engine::Stem::Vocals),
        "drums" => Some(od_engine::Stem::Drums),
        "bass" => Some(od_engine::Stem::Bass),
        "inst" => Some(od_engine::Stem::Inst),
        _ => None,
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    let zoom = crate::preferences::default_deck_zoom(&app.state.borrow().settings);
    app.decks.borrow_mut().zoom = [zoom; DECKS];
    let ui = &app.ui;
    ui.set_lane_motion(ModelRc::from(app.decks.borrow().motion.clone()));
    ui.on_deck_zoom(handler!(app, |a, d: i32, delta: f32| {
        let i = (d as usize).saturating_sub(1).min(DECKS - 1);
        let mut decks = a.decks.borrow_mut();
        decks.zoom[i] = zoom_by(decks.zoom[i], delta as f64);
    }));
    ui.on_deck_zoom_reset(handler!(app, |a, d: i32| {
        let i = (d as usize).saturating_sub(1).min(DECKS - 1);
        let zoom = crate::preferences::default_deck_zoom(&a.state.borrow().settings);
        a.decks.borrow_mut().zoom[i] = zoom;
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
    ui.on_deck_stem_level(handler!(app, |a, d: i32, stem: SharedString, level: f32| {
        engine::ensure_engine(&a);
        if let Some(s) = stem_of_ui(stem.as_str()) {
            engine::deck_cmd(&a, idx(d), DeckCommand::SetStemLevel { stem: s, level: level.clamp(0.0, 1.0) });
        }
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
    ui.on_deck_pad(handler!(app, |a, d: i32, i: i32, pressed: bool| pad(&a, idx(d), i.max(0) as usize, pressed)));
    ui.on_deck_pad_alt(handler!(app, |a, d: i32, i: i32| pad_alt(&a, idx(d), i.max(0) as usize)));
    ui.on_deck_pad_mode_step(handler!(app, |a, d: i32, x: i32| {
        let mode = pads::step_mode(a.decks.borrow().pad_mode[idx(d)], x);
        set_pad_mode(&a, idx(d), mode);
    }));
    ui.on_deck_pad_mode_set(handler!(app, |a, d: i32, i: i32| set_pad_mode(&a, idx(d), pads::UI_MODES[(i.max(0) as usize).min(pads::UI_MODES.len() - 1)])));
    ui.on_deck_pad_param(handler!(app, |a, d: i32, x: i32| pad_param(&a, idx(d), x)));
    ui.on_deck_key_sync(handler!(app, |a, d: i32| toggle_key_sync(&a, idx(d))));
    ui.on_deck_key_shift(handler!(app, |a, d: i32, x: i32| key_shift(&a, idx(d), x)));
    ui.on_deck_beat_jump_step(handler!(app, |a, d: i32, fwd: bool| {
        engine::ensure_engine(&a);
        engine::deck_cmd(&a, idx(d), DeckCommand::BeatJumpStep { forward: fwd });
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
    fn hot_cue_events_edit_the_cue_list() {
        let mut named = od_core::Cue::hot(2, 10.0);
        named.label = "Drop".into();
        let cues = vec![named, od_core::Cue::hot(0, 1.0)];
        // Moving C keeps its label; setting B adds it; deleting A removes it.
        let moved = apply_hot_cue(cues, 2, Some(od_engine::HotCue { start: 12.5, loop_end: None }));
        let c = moved.iter().find(|c| c.kind == CueKind::Hot(2)).unwrap();
        assert_eq!((c.start, c.label.as_str()), (12.5, "Drop"));
        let added = apply_hot_cue(moved, 1, Some(od_engine::HotCue { start: 4.0, loop_end: Some(6.0) }));
        let b = added.iter().find(|c| c.kind == CueKind::Hot(1)).unwrap();
        assert_eq!((b.start, b.loop_end), (4.0, Some(6.0)));
        let removed = apply_hot_cue(added, 0, None);
        assert!(!removed.iter().any(|c| c.kind == CueKind::Hot(0)));
        assert_eq!(removed.len(), 2);
    }

    #[test]
    fn memory_cue_edits() {
        let cues = vec![od_core::Cue::hot(0, 1.0), od_core::Cue::memory(10.0)];
        let added = edit_memory_cue(cues.clone(), 20.0, true);
        assert_eq!(added.len(), 3);
        // No duplicate at the same spot.
        assert_eq!(edit_memory_cue(added.clone(), 20.01, true).len(), 3);
        // Delete takes the nearest memory cue within a second, never a hot cue.
        let removed = edit_memory_cue(added, 10.4, false);
        assert!(!removed.iter().any(|c| c.kind == CueKind::Memory && c.start == 10.0));
        assert_eq!(edit_memory_cue(removed.clone(), 1.0, false), removed);
        // MEMORY while looping saves the loop.
        let mut with_loop = edit_memory_cue(Vec::new(), 32.0, true);
        set_memory_loop_end(&mut with_loop, 32.0, 36.0);
        assert_eq!(with_loop[0].loop_end, Some(36.0));
        assert!(with_loop[0].is_loop());
    }

    #[test]
    fn tapped_grid_keeps_the_downbeat() {
        let g = Beatgrid::constant(120.0, 0.25, 3, 60.0);
        let first_down = g.beats.iter().find(|b| b.bar_position == 1).unwrap().time;
        let t = tapped_grid(&g, 125.0, 60.0);
        assert!(t.beats.iter().any(|b| b.bar_position == 1 && (b.time - first_down).abs() < 1e-9));
        assert!((t.beats[1].time - t.beats[0].time - 60.0 / 125.0).abs() < 1e-9);
        assert!(!tapped_grid(&Beatgrid::default(), 128.0, 30.0).beats.is_empty());
    }

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
        let f = deck_fields(&s, &DeckInputs { deck: 1, meta: None, level: 0.0, fader: 1.0, quantize: true, stems_busy: None, loop_size: 0.5, bar_counter: "up", pad_mode: PadMode::HotCue, pad_param: String::new() });
        assert_eq!(f.deck, 2);
        assert!(!f.loaded);
        assert_eq!(f.bpm, "");
        assert_eq!(f.remaining, "");
        assert_eq!(f.loop_size, "1/2");
        assert_eq!(f.elapsed, "0:00.0");
        // Unchanged state projects equal, so no row is written.
        let g = deck_fields(&s, &DeckInputs { deck: 1, meta: None, level: 0.0, fader: 1.0, quantize: true, stems_busy: None, loop_size: 0.5, bar_counter: "up", pad_mode: PadMode::HotCue, pad_param: String::new() });
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
    fn split_progress_text() {
        assert_eq!(split_text(None), "");
        assert_eq!(split_text(Some(0.0)), "SPLIT…");
        assert_eq!(split_text(Some(0.423)), "SPLIT 42%");
        assert_eq!(split_text(Some(1.0)), "SPLIT 99%");
    }

    #[test]
    fn stick_tracks_use_the_stems_on_the_stick() {
        let root = std::path::Path::new("/Volumes/STICK");
        let cache = stick_stem_cache(root);
        assert_eq!(cache.dir(), std::path::Path::new("/Volumes/STICK/OPENDECKS/stems"));
        // Same file name as the sync writes (od-devices `stem_file`).
        let hash = od_core::ContentHash("ab12".into());
        assert_eq!(cache.path(&hash), std::path::Path::new("/Volumes/STICK/OPENDECKS/stems/ab12.odstem"));
    }

    #[test]
    fn tempo_ranges() {
        assert_eq!(tempo_range_label(TempoRange::Ten), "±10%");
        assert_eq!(tempo_range_label(TempoRange::Wide), "WIDE");
    }
}
