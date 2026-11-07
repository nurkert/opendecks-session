//! Prepare player: the selected track's waveforms, cursor, hot / memory
//! cues, grid editing and the preview (played on the hidden preview deck).

use crate::app::{App, handler};
use crate::models::model;
use crate::{CueMark, HotCueSlot, PhraseMark, decks, engine, format, render};
use od_core::{Beatgrid, Cue, CueKind, TrackId};
use od_engine::{ChannelCommand, Command, DeckCommand};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

#[derive(Default)]
pub(crate) struct PlayerState {
    /// The track shown in the player (and loaded by LOAD / double-click).
    pub current: Option<TrackId>,
    /// Cursor in the selected track, seconds.
    pub cursor: f64,
    /// The preview deck is playing the selected track.
    pub previewing: bool,
    /// TAP in the grid editor: times of the taps so far (seconds).
    pub taps: Vec<f64>,
    /// The second player of the 2-player and dual layouts, its cursor.
    pub second: Option<TrackId>,
    pub second_cursor: f64,
    /// The player the preview deck plays for (1 or 2).
    pub preview_slot: u8,
}

/// Taps further apart than this start a new count (seconds).
const TAP_GAP: f64 = 2.0;
/// Taps needed before a tempo is set.
pub(crate) const TAPS_NEEDED: usize = 4;

/// Adds a tap at `t` (seconds) and returns the tapped tempo once there are
/// enough taps: the mean interval of the last 16 taps (pure).
pub(crate) fn register_tap(taps: &mut Vec<f64>, t: f64) -> Option<f64> {
    if taps.last().is_some_and(|l| t - l > TAP_GAP || t <= *l) {
        taps.clear();
    }
    taps.push(t);
    if taps.len() > 16 {
        taps.remove(0);
    }
    if taps.len() < TAPS_NEEDED {
        return None;
    }
    let span = taps[taps.len() - 1] - taps[0];
    let bpm = 60.0 * (taps.len() - 1) as f64 / span;
    (30.0..=300.0).contains(&bpm).then(|| (bpm * 100.0).round() / 100.0)
}

/// A typed tempo ("128", "127,5", "124.00 BPM"), 30–300 BPM (pure).
pub(crate) fn parse_bpm(text: &str) -> Option<f64> {
    let t = text.trim().trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace()).replace(',', ".");
    let bpm: f64 = t.parse().ok()?;
    (30.0..=300.0).contains(&bpm).then(|| (bpm * 100.0).round() / 100.0)
}

/// A constant grid at `bpm` that keeps the first downbeat of `grid` (an
/// empty grid starts at 0 s) and its lock (pure).
pub(crate) fn grid_at_bpm(grid: &Beatgrid, bpm: f64, duration: f64) -> Beatgrid {
    let anchor = grid.beats.iter().find(|b| b.bar_position == 1).or(grid.beats.first()).map(|b| b.time).unwrap_or(0.0);
    let mut g = Beatgrid::constant(bpm, anchor, 1, duration.max(anchor + 1.0));
    g.locked = grid.locked;
    g
}

/// Seconds since the first call (monotonic), for the tap tempo.
fn now_secs() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// TAP in the grid editor: after four taps the grid takes the tapped tempo
/// (keeping its first downbeat); every further tap refines it.
pub(crate) fn tap(app: &App) {
    tap_at(app, now_secs());
}

pub(crate) fn tap_at(app: &App, t: f64) {
    let bpm = register_tap(&mut app.state.borrow_mut().player.taps, t);
    let n = app.state.borrow().player.taps.len();
    match bpm {
        Some(bpm) => set_grid_bpm(app, bpm, &od_i18n::trf("Tapped tempo: {} BPM", &[&format!("{bpm:.2}")])),
        None => app.status(od_i18n::trf("Tap {} of {}", &[&n, &TAPS_NEEDED])),
    }
}

/// The BPM field of the grid editor (Return).
pub(crate) fn type_bpm(app: &App, text: &str) {
    match parse_bpm(text) {
        Some(bpm) => set_grid_bpm(app, bpm, &od_i18n::trf("Beatgrid set to {} BPM", &[&format!("{bpm:.2}")])),
        None => {
            app.status("Type a tempo between 30 and 300 BPM");
            refresh_detail(app);
        }
    }
}

/// Replaces the current track's grid by a constant one at `bpm`.
fn set_grid_bpm(app: &App, bpm: f64, label: &str) {
    if app.guard_read_only() {
        return;
    }
    let Some(id) = app.state.borrow().player.current else { return };
    let (grid, duration) = {
        let st = app.state.borrow();
        (st.lib.beatgrid(id).ok().flatten().unwrap_or_default(), st.lib.track(id).map(|t| t.duration).unwrap_or(0.0))
    };
    if grid.locked {
        app.status("The beatgrid is locked — unlock it to edit");
        return;
    }
    let r = app.state.borrow_mut().lib.set_beatgrid(id, grid_at_bpm(&grid, bpm, duration));
    if app.report(r).is_some() {
        app.after_edit(label);
    }
}

/// Cue markers as fractions of the track; `with_time` adds the cue time.
pub(crate) fn cue_marks(cues: &[Cue], duration: f64, with_time: bool) -> Vec<CueMark> {
    let duration = duration.max(0.001);
    cues.iter()
        .map(|c| CueMark {
            slot: match c.kind {
                CueKind::Hot(s) => s as i32,
                CueKind::Memory => -1,
            },
            letter: c.kind.hot_slot_letter().map(|l| l.to_string()).unwrap_or_default().into(),
            pos: (c.start / duration) as f32,
            end: (c.loop_end.unwrap_or(c.start) / duration) as f32,
            is_loop: c.is_loop(),
            active: c.active && c.is_loop(),
            time: if with_time { format::cue_time(c.start).into() } else { Default::default() },
            label: c.label.clone().into(),
            tone: cue_tone(c.color),
        })
        .collect()
}

/// A stored cue colour for the views (transparent = none).
pub(crate) fn cue_tone(c: Option<od_core::CueColor>) -> slint::Color {
    c.map(|c| slint::Color::from_rgb_u8(c.r, c.g, c.b)).unwrap_or(slint::Color::from_argb_u8(0, 0, 0, 0))
}

/// Cue colours offered in the cue editor (the eight colours compatible
/// players can show).
pub(crate) const CUE_PALETTE: [od_core::CueColor; 8] = [
    od_core::CueColor::rgb(0xE6, 0x28, 0x28),
    od_core::CueColor::rgb(0xFF, 0x80, 0x00),
    od_core::CueColor::rgb(0xF5, 0xCE, 0x1E),
    od_core::CueColor::rgb(0x28, 0xE2, 0x14),
    od_core::CueColor::rgb(0x10, 0xB1, 0x76),
    od_core::CueColor::rgb(0x30, 0x5A, 0xFF),
    od_core::CueColor::rgb(0xB4, 0x32, 0xFF),
    od_core::CueColor::rgb(0xDE, 0x44, 0xCF),
];

/// One cue-list edit (pure): `index` into the cue list as shown.
pub(crate) enum CueEditOp {
    Label(String),
    Color(usize),
    MoveTo(f64),
    ToHot,
    /// Active loop on / off (loops only).
    ToggleActive,
    Delete,
}

pub(crate) fn apply_cue_edit(cues: &mut Vec<Cue>, index: usize, op: CueEditOp) -> bool {
    if index >= cues.len() {
        return false;
    }
    match op {
        CueEditOp::Label(l) => cues[index].label = l.trim().to_string(),
        CueEditOp::Color(i) => cues[index].color = CUE_PALETTE.get(i).copied(),
        CueEditOp::MoveTo(t) => {
            let c = &mut cues[index];
            if let Some(end) = c.loop_end {
                c.loop_end = Some(t + (end - c.start));
            }
            c.start = t.max(0.0);
        }
        CueEditOp::ToHot => {
            let used: Vec<u8> = cues.iter().filter_map(|c| match c.kind {
                CueKind::Hot(s) => Some(s),
                CueKind::Memory => None,
            }).collect();
            let Some(free) = (0..od_core::cue::HOT_CUE_SLOTS).find(|s| !used.contains(s)) else { return false };
            if cues[index].kind != CueKind::Memory {
                return false;
            }
            cues[index].kind = CueKind::Hot(free);
        }
        CueEditOp::ToggleActive => {
            if !cues[index].is_loop() {
                return false;
            }
            cues[index].active = !cues[index].active;
        }
        CueEditOp::Delete => {
            cues.remove(index);
        }
    }
    true
}

fn cue_edit(app: &App, index: i32, label: &str, op: CueEditOp) {
    if index < 0 {
        return;
    }
    let mut ok = false;
    edit_cues(app, label, |cues, _| ok = apply_cue_edit(cues, index as usize, op));
    if !ok {
        app.status("No free hot cue pad");
    }
}

/// The eight hot cue slots A–H.
pub(crate) fn hot_cue_slots(cues: &[Cue]) -> Vec<HotCueSlot> {
    (0..8u8)
        .map(|s| {
            let c = cues.iter().find(|c| c.kind == CueKind::Hot(s));
            HotCueSlot {
                slot: s as i32,
                letter: ((b'A' + s) as char).to_string().into(),
                set: c.is_some(),
                time: c.map(|c| format::cue_time(c.start)).unwrap_or_default().into(),
                label: c.map(|c| c.label.clone()).unwrap_or_default().into(),
                tone: cue_tone(c.and_then(|c| c.color)),
            }
        })
        .collect()
}

/// Phrase blocks as fractions of the track, for the phrase bars.
pub(crate) fn phrase_marks(phrases: &[od_core::Phrase], grid: &Beatgrid, duration: f64) -> Vec<PhraseMark> {
    let at = |beat: u32| grid.beats.get(beat as usize).map(|b| b.time).unwrap_or(duration);
    phrases
        .iter()
        .filter(|_| duration > 0.0)
        .map(|p| {
            let [r, g, b] = render::phrase_rgb(p.kind);
            PhraseMark {
                pos: (at(p.start_beat) / duration) as f32,
                end: (at(p.end_beat) / duration).min(1.0) as f32,
                label: render::phrase_label(p.kind).into(),
                tone: slint::Color::from_rgb_u8(r, g, b),
            }
        })
        .collect()
}

/// Moves the cursor to the nearest beat when within 80 ms of it.
pub(crate) fn snap_to_beat(grid: &Beatgrid, t: f64) -> f64 {
    grid.beat_position(t).and_then(|b| grid.time_of_beat(b.round())).filter(|s| (s - t).abs() < 0.08).unwrap_or(t)
}

pub(crate) fn refresh_detail(app: &App) {
    crate::browser::refresh_info(app);
    let st = app.state.borrow();
    let Some(id) = st.player.current else {
        app.ui.set_has_track(false);
        drop(st);
        refresh_minis(app);
        return;
    };
    let Ok(t) = st.src().track(id) else {
        app.ui.set_has_track(false);
        drop(st);
        refresh_minis(app);
        return;
    };
    let ui = &app.ui;
    let analysis = st.src().analysis(id).unwrap_or_default();
    let grid = st.src().beatgrid_with_source(id).ok().flatten();
    let cues = st.src().cues(id).unwrap_or_default();
    let duration = t.duration.max(0.001);
    ui.set_has_track(true);
    ui.set_track_title(t.tags.title.clone().into());
    ui.set_track_artist(t.tags.artist.clone().into());
    ui.set_track_bpm(t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default().into());
    ui.set_track_key(st.key_text(&t).into());
    ui.set_track_duration(format::time(t.duration).into());
    ui.set_track_analysed(t.analysis_version >= od_analysis::ANALYSIS_VERSION);
    ui.set_grid_source(od_i18n::tr(&grid.as_ref().map(|(_, s)| s.as_str().to_uppercase()).unwrap_or_else(|| "NONE".into())).to_string().into());
    ui.set_grid_locked(grid.as_ref().is_some_and(|(g, _)| g.locked));
    let scale = ui.window().scale_factor();
    ui.set_track_overview(render::overview(&analysis.waveform, &analysis.beatgrid, t.duration, (1000.0 * scale) as u32, (48.0 * scale) as u32));
    ui.set_track_phrases(model(phrase_marks(&analysis.phrases, &analysis.beatgrid, t.duration)));
    ui.set_track_cues(model(cue_marks(&cues, t.duration, true)));
    let slots = hot_cue_slots(&cues);
    ui.set_hot_cues_top(model(slots[..4].to_vec()));
    ui.set_hot_cues_bottom(model(slots[4..].to_vec()));
    ui.set_hot_cues(model(slots));
    ui.set_cursor((st.player.cursor / duration) as f32);
    ui.set_track_position(format::cue_time(st.player.cursor).into());
    let phrases = if st.settings.view.phrases { analysis.phrases.clone() } else { Vec::new() };
    DETAIL_SRC.with(|d| *d.borrow_mut() = Some(DetailSrc { track: id, waveform: analysis.waveform, grid: analysis.beatgrid, phrases, last_px: None }));
    drop(st);
    render_detail(app);
    refresh_minis(app);
}

// ── Compact players (2 players, dual, simple) ───────────────────────────

thread_local! {
    /// Overview of the second player's track, rendered once per track.
    static SECOND_OVERVIEW: std::cell::RefCell<Option<(TrackId, slint::Image)>> = const { std::cell::RefCell::new(None) };
}

/// The compact player views: 1 = the selected track, 2 = the second player.
pub(crate) fn refresh_minis(app: &App) {
    let st = app.state.borrow();
    let p = &st.player;
    let view = |id: Option<TrackId>, cursor: f64, previewing: bool, overview: Option<slint::Image>| -> crate::MiniView {
        let Some(t) = id.and_then(|id| st.src().track(id).ok()) else { return crate::MiniView::default() };
        let overview = overview.unwrap_or_else(|| {
            let a = st.src().analysis(t.id).unwrap_or_default();
            render::overview(&a.waveform, &a.beatgrid, t.duration, 900, 48)
        });
        crate::MiniView {
            has_track: true,
            title: t.tags.title.clone().into(),
            artist: t.tags.artist.clone().into(),
            bpm: t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default().into(),
            key: st.key_text(&t).into(),
            position: format::cue_time(cursor).into(),
            overview,
            cursor: (cursor / t.duration.max(0.001)) as f32,
            previewing,
        }
    };
    let one = view(p.current, p.cursor, p.previewing && p.preview_slot != 2, Some(app.ui.get_track_overview()));
    let cached = SECOND_OVERVIEW.with(|c| c.borrow().as_ref().filter(|(t, _)| Some(*t) == p.second).map(|(_, i)| i.clone()));
    let two = view(p.second, p.second_cursor, p.previewing && p.preview_slot == 2, cached);
    if let Some(t) = p.second {
        SECOND_OVERVIEW.with(|c| *c.borrow_mut() = Some((t, two.overview.clone())));
    }
    drop(st);
    app.ui.set_mini1(if one.has_track { one } else { crate::MiniView::default() });
    app.ui.set_mini2(two);
}

/// Loads `track` into the second player.
pub(crate) fn load_second(app: &App, track: TrackId) {
    {
        let mut st = app.state.borrow_mut();
        if st.player.previewing && st.player.preview_slot == 2 {
            drop(st);
            engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Pause);
            set_previewing(app, false);
            st = app.state.borrow_mut();
        }
        st.player.second = Some(track);
        st.player.second_cursor = 0.0;
    }
    refresh_minis(app);
}

/// The selected track of the browser (library tracks only).
fn selected_track(app: &App) -> Option<TrackId> {
    let st = app.state.borrow();
    let i = st.browser.selection.iter().next_back().copied()?;
    if st.explorer.open.is_some() { st.explorer.track_at(i) } else { st.browser.rows.get(i).map(|t| t.id) }
}

/// Play / pause the second player on the preview deck.
fn toggle_second(app: &App) {
    let (track, cursor, playing) = {
        let st = app.state.borrow();
        (st.player.second, st.player.second_cursor, st.player.previewing && st.player.preview_slot == 2)
    };
    let Some(track) = track else { return };
    if playing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Pause);
        set_previewing(app, false);
        return;
    }
    if app.state.borrow().player.previewing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Pause);
    }
    app.state.borrow_mut().player.preview_slot = 2;
    if app.decks.borrow().loaded_id(decks::PREVIEW_DECK) == Some(track) {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Seek { seconds: cursor });
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Play);
    } else {
        decks::load_track(app, decks::PREVIEW_DECK, track, Some(cursor));
    }
    set_previewing(app, true);
    engine::send(app, Command::Channel(decks::PREVIEW_DECK, ChannelCommand::SetFader(1.0)));
}

/// Moves the second player's cursor (and the preview deck when it plays it).
fn seek_second(app: &App, seconds: f64) {
    let playing = {
        let mut st = app.state.borrow_mut();
        st.player.second_cursor = seconds.max(0.0);
        st.player.previewing && st.player.preview_slot == 2
    };
    if playing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Seek { seconds });
    }
    refresh_minis(app);
}

/// A compact player button: "play", "cue" or "load" on player `n`.
pub(crate) fn mini_act(app: &App, n: i32, what: &str) {
    match (n, what) {
        (1, "play") => toggle_preview(app),
        (1, "cue") => app.ui.invoke_jump_start(),
        (2, "play") => toggle_second(app),
        (2, "cue") => seek_second(app, 0.0),
        (2, "load") => match selected_track(app) {
            Some(t) => load_second(app, t),
            None => app.status("Select a track in the list first"),
        },
        _ => {}
    }
}

/// A click on a compact player's overview (0..1).
pub(crate) fn mini_seek(app: &App, n: i32, p: f32) {
    if n == 1 {
        app.ui.invoke_seek(p);
        return;
    }
    let d = {
        let st = app.state.borrow();
        st.player.second.and_then(|id| st.src().track(id).ok()).map(|t| t.duration).unwrap_or(0.0)
    };
    seek_second(app, p as f64 * d);
}

/// What the zoomed waveform is drawn from, kept so playback can scroll it
/// every frame without touching the library.
struct DetailSrc {
    track: TrackId,
    waveform: od_core::Waveform,
    grid: Beatgrid,
    phrases: Vec<od_core::Phrase>,
    /// Cursor in output pixels at the last render (skip identical frames).
    last_px: Option<i64>,
}

thread_local! {
    static DETAIL_SRC: std::cell::RefCell<Option<DetailSrc>> = const { std::cell::RefCell::new(None) };
}

/// Renders the zoomed waveform around the cursor (only when it moved by at
/// least one output pixel).
fn render_detail(app: &App) {
    let ui = &app.ui;
    let scale = ui.window().scale_factor();
    let width = ((ui.window().size().width as f32 / scale - 680.0).max(300.0) * scale) as u32;
    let (cursor, current, span) = {
        let st = app.state.borrow();
        (st.player.cursor, st.player.current, st.settings.view.zoom_span as f64)
    };
    DETAIL_SRC.with(|d| {
        let mut d = d.borrow_mut();
        let Some(src) = d.as_mut().filter(|s| Some(s.track) == current) else { return };
        let px = (cursor / span * width as f64).round() as i64;
        if src.last_px == Some(px) {
            return;
        }
        src.last_px = Some(px);
        ui.set_track_detail(render::detail(&src.waveform, &src.grid, &src.phrases, cursor, span, width, (150.0 * scale) as u32));
    });
}

/// Space / play button: start or pause the preview of the selected track.
pub(crate) fn toggle_preview(app: &App) {
    let previewing = app.state.borrow().player.previewing && app.state.borrow().player.preview_slot != 2;
    if app.state.borrow().player.preview_slot == 2 && app.state.borrow().player.previewing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Pause);
        set_previewing(app, false);
    }
    app.state.borrow_mut().player.preview_slot = 1;
    if previewing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Pause);
        set_previewing(app, false);
        return;
    }
    let (track, cursor) = {
        let st = app.state.borrow();
        (st.player.current, st.player.cursor)
    };
    let Some(track) = track else { return };
    let already = app.decks.borrow().loaded_id(decks::PREVIEW_DECK) == Some(track);
    if already {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Seek { seconds: cursor });
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Play);
    } else {
        decks::load_track(app, decks::PREVIEW_DECK, track, Some(cursor));
    }
    set_previewing(app, true);
    // The preview deck is audible on the master at full level.
    engine::send(app, Command::Channel(decks::PREVIEW_DECK, ChannelCommand::SetFader(1.0)));
}

fn set_previewing(app: &App, on: bool) {
    app.state.borrow_mut().player.previewing = on;
    let slot = app.state.borrow().player.preview_slot;
    app.ui.set_previewing(on && slot != 2);
    refresh_minis(app);
}

/// Follows the cursor to the preview deck after a seek.
fn preview_seek(app: &App) {
    let (previewing, cursor) = {
        let st = app.state.borrow();
        (st.player.previewing, st.player.cursor)
    };
    if previewing {
        engine::deck_cmd(app, decks::PREVIEW_DECK, DeckCommand::Seek { seconds: cursor });
    }
}

/// Frame tick: the cursor follows the preview deck.
pub(crate) fn follow_preview(app: &App, state: &od_engine::EngineState) {
    if !app.state.borrow().player.previewing {
        return;
    }
    let d = &state.decks[decks::PREVIEW_DECK];
    if app.state.borrow().player.preview_slot == 2 {
        let second = app.state.borrow().player.second;
        if d.track == second && d.track.is_some() {
            app.state.borrow_mut().player.second_cursor = d.position;
            refresh_minis(app);
        }
        if d.at_end {
            set_previewing(app, false);
        }
        return;
    }
    let current = app.state.borrow().player.current;
    if d.track == current && d.track.is_some() {
        app.state.borrow_mut().player.cursor = d.position;
        if d.duration > 0.0 {
            app.ui.set_cursor((d.position / d.duration) as f32);
        }
        app.ui.set_track_position(format::cue_time(d.position).into());
        render_detail(app);
    }
    if d.at_end {
        set_previewing(app, false);
    }
}

fn move_cursor(app: &App, f: impl FnOnce(&mut crate::state::AppState)) {
    f(&mut app.state.borrow_mut());
    preview_seek(app);
    refresh_detail(app);
}

fn edit_grid(app: &App, f: impl FnOnce(&mut Beatgrid, f64)) {
    if app.guard_read_only() {
        return;
    }
    let (id, cursor) = {
        let st = app.state.borrow();
        match st.player.current {
            Some(id) => (id, st.player.cursor),
            None => return,
        }
    };
    let grid = app.state.borrow().lib.beatgrid(id).ok().flatten();
    let Some(mut grid) = grid else {
        app.status("This track has no beatgrid yet — analyse it first");
        return;
    };
    f(&mut grid, cursor);
    let r = app.state.borrow_mut().lib.set_beatgrid(id, grid);
    if app.report(r).is_some() {
        app.after_edit("");
    }
}

/// Moves the cursor to the next (or previous) cue of the current track.
pub(crate) fn step_cue(app: &App, forward: bool) {
    move_cursor(app, |st| {
        let Some(id) = st.player.current else { return };
        let cues = st.src().cues(id).unwrap_or_default();
        let target = next_cue(cues.iter().map(|c| c.start), st.player.cursor, forward);
        if let Some(t) = target {
            st.player.cursor = t;
        }
    });
}

/// The cue start after (or before) `cursor`, ignoring cues within 5 ms.
pub(crate) fn next_cue(starts: impl Iterator<Item = f64>, cursor: f64, forward: bool) -> Option<f64> {
    let starts = starts.filter(|t| (t - cursor).abs() > 0.005);
    if forward { starts.filter(|t| *t > cursor).reduce(f64::min) } else { starts.filter(|t| *t < cursor).reduce(f64::max) }
}

/// Deletes the memory cue at the cursor (within 50 ms).
pub(crate) fn delete_memory_cue(app: &App) {
    edit_cues(app, "Memory cue removed", |cues, cursor| cues.retain(|c| !(c.kind == CueKind::Memory && (c.start - cursor).abs() < 0.05)));
}

/// Replaces the cues of the current track (library only).
fn edit_cues(app: &App, label: &str, f: impl FnOnce(&mut Vec<Cue>, f64)) {
    if app.guard_read_only() {
        return;
    }
    let (id, cursor) = {
        let st = app.state.borrow();
        match st.player.current {
            Some(id) => (id, st.player.cursor),
            None => return,
        }
    };
    let mut cues = app.state.borrow().lib.cues(id).unwrap_or_default();
    f(&mut cues, cursor);
    let r = app.state.borrow_mut().lib.set_cues(id, cues);
    if app.report(r).is_some() {
        app.after_edit(label);
    }
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.on_mini_act(handler!(app, |a, n: i32, w: SharedString| mini_act(&a, n, w.as_str())));
    ui.on_mini_seek(handler!(app, |a, n: i32, p: f32| mini_seek(&a, n, p)));
    let ce = ui.global::<crate::CueEdit>();
    ce.set_palette(model(CUE_PALETTE.iter().map(|c| cue_tone(Some(*c))).collect()));
    ce.on_label(handler!(app, |a, i: i32, t: SharedString| cue_edit(&a, i, "Cue named", CueEditOp::Label(t.to_string()))));
    ce.on_color(handler!(app, |a, i: i32, c: i32| cue_edit(&a, i, "Cue colour changed", CueEditOp::Color(c.max(0) as usize))));
    ce.on_move_to_cursor(handler!(app, |a, i: i32| {
        let cursor = {
            let st = a.state.borrow();
            let grid = st.player.current.and_then(|id| st.src().beatgrid(id).ok().flatten());
            grid.map(|g| snap_to_beat(&g, st.player.cursor)).unwrap_or(st.player.cursor)
        };
        cue_edit(&a, i, "Cue moved", CueEditOp::MoveTo(cursor))
    }));
    ce.on_to_hot(handler!(app, |a, i: i32| cue_edit(&a, i, "Memory cue made a hot cue", CueEditOp::ToHot)));
    ce.on_delete(handler!(app, |a, i: i32| cue_edit(&a, i, "Cue removed", CueEditOp::Delete)));
    ce.on_active(handler!(app, |a, i: i32| cue_edit(&a, i, "Active loop changed", CueEditOp::ToggleActive)));
    ui.on_seek(handler!(app, |a, p: f32| move_cursor(&a, |st| {
        let id = st.player.current;
        let d = id.and_then(|id| st.src().track(id).ok()).map(|t| t.duration).unwrap_or(0.0);
        let t = p as f64 * d;
        let grid = id.and_then(|id| st.src().beatgrid(id).ok().flatten());
        st.player.cursor = grid.map(|g| snap_to_beat(&g, t)).unwrap_or(t);
    })));
    ui.on_nudge(handler!(app, |a, d: f32| move_cursor(&a, |st| {
        let dur = st.player.current.and_then(|id| st.src().track(id).ok()).map(|t| t.duration).unwrap_or(0.0);
        st.player.cursor = (st.player.cursor + d as f64).clamp(0.0, dur);
    })));
    // CUE: back to the first memory cue, or the start.
    ui.on_jump_start(handler!(app, |a| move_cursor(&a, |st| {
        let first = st
            .player
            .current
            .and_then(|id| st.src().cues(id).ok())
            .and_then(|c| c.iter().filter(|c| c.kind == CueKind::Memory).map(|c| c.start).reduce(f64::min));
        st.player.cursor = first.unwrap_or(0.0);
    })));
    ui.on_hot_cue(handler!(app, |a, slot: i32| {
        let Some(id) = a.state.borrow().player.current else { return };
        let cues = a.state.borrow().src().cues(id).unwrap_or_default();
        if let Some(c) = cues.iter().find(|c| c.kind == CueKind::Hot(slot as u8)) {
            let start = c.start;
            move_cursor(&a, |st| st.player.cursor = start);
        } else {
            edit_cues(&a, &format!("Hot cue {} set", (b'A' + slot as u8) as char), |cues, cursor| cues.push(Cue::hot(slot as u8, cursor)));
        }
    }));
    ui.on_remove_hot_cue(handler!(app, |a, slot: i32| edit_cues(&a, "Hot cue removed", |cues, _| cues.retain(|c| c.kind != CueKind::Hot(slot as u8)))));
    ui.on_add_memory_cue(handler!(app, |a| edit_cues(&a, "Memory cue added", |cues, cursor| cues.push(Cue::memory(cursor)))));
    ui.on_grid_shift(handler!(app, |a, ms: i32| edit_grid(&a, |g, _| g.shift(ms as f64 / 1000.0))));
    ui.on_grid_scale(handler!(app, |a, f: f32| edit_grid(&a, |g, _| {
        let Some(first) = g.beats.first().copied() else { return };
        let bpm = g.dominant_bpm().unwrap_or(first.bpm) * f as f64;
        let end = g.beats.last().map(|b| b.time).unwrap_or(0.0) + 1.0;
        *g = Beatgrid::constant(bpm, first.time, first.bar_position, end);
    })));
    ui.on_grid_downbeat(handler!(app, |a| edit_grid(&a, |g, cursor| {
        if let Some(i) = g.beat_position(cursor).map(|b| b.round().max(0.0) as usize) {
            g.set_downbeat(i.min(g.beats.len().saturating_sub(1)));
        }
    })));
    ui.on_grid_lock(handler!(app, |a| edit_grid(&a, |g, _| g.locked = !g.locked)));
    ui.on_grid_tap(handler!(app, |a| tap(&a)));
    ui.on_grid_bpm(handler!(app, |a, t: SharedString| type_bpm(&a, t.as_str())));
    ui.on_analyse_selected(handler!(app, |a| {
        if a.guard_read_only() {
            return;
        }
        let id = a.state.borrow().player.current;
        if let Some(id) = id {
            let _ = a.state.borrow().lib.conn().execute("UPDATE track SET analysis_version = 0 WHERE id = ?1", [id.0]);
            // Also when automatic analysis is off (Preferences > Analysis).
            a.analysis.control.lock().unwrap_or_else(|e| e.into_inner()).requested.push(id);
            let _ = a.analysis.wake.send(());
            a.status("Queued for analysis");
        }
    }));
    ui.on_toggle_preview(handler!(app, |a| toggle_preview(&a)));
}

#[cfg(test)]
mod tests {

    #[test]
    fn cue_list_edits() {
        let mut cues = vec![Cue::hot(0, 10.0), Cue::memory(20.0), Cue { loop_end: Some(34.0), ..Cue::memory(30.0) }];
        assert!(apply_cue_edit(&mut cues, 0, CueEditOp::Label("  Drop ".into())));
        assert_eq!(cues[0].label, "Drop");
        assert!(apply_cue_edit(&mut cues, 1, CueEditOp::Color(0)));
        assert_eq!(cues[1].color, Some(CUE_PALETTE[0]));
        assert!(apply_cue_edit(&mut cues, 2, CueEditOp::MoveTo(40.0)));
        assert_eq!((cues[2].start, cues[2].loop_end), (40.0, Some(44.0)), "a loop keeps its length");
        assert!(apply_cue_edit(&mut cues, 1, CueEditOp::ToHot));
        assert_eq!(cues[1].kind, CueKind::Hot(1), "first free pad");
        assert!(!apply_cue_edit(&mut cues, 0, CueEditOp::ToHot), "already a hot cue");
        assert!(apply_cue_edit(&mut cues, 2, CueEditOp::ToggleActive));
        assert!(cues[2].active, "a loop becomes an active loop");
        assert!(!apply_cue_edit(&mut cues, 0, CueEditOp::ToggleActive), "a plain cue cannot be active");
        assert!(cue_marks(&cues, 60.0, false)[2].active);
        assert!(apply_cue_edit(&mut cues, 2, CueEditOp::Delete));
        assert_eq!(cues.len(), 2);
        assert!(!apply_cue_edit(&mut cues, 9, CueEditOp::Delete));
        assert_eq!(cue_tone(None).alpha(), 0);
    }

    use super::*;

    #[test]
    fn cue_projection() {
        let cues = vec![Cue::hot(1, 30.0), Cue::memory(15.0)];
        let marks = cue_marks(&cues, 60.0, true);
        assert_eq!(marks[0].slot, 1);
        assert_eq!(marks[0].letter, "B");
        assert_eq!(marks[0].pos, 0.5);
        assert_eq!(marks[0].time, "0:30.000");
        assert_eq!(marks[1].slot, -1);
        assert_eq!(cue_marks(&cues, 60.0, false)[0].time, "");
        let slots = hot_cue_slots(&cues);
        assert_eq!(slots.len(), 8);
        assert!(!slots[0].set && slots[1].set);
        assert_eq!(slots[7].letter, "H");
    }

    #[test]
    fn cue_stepping() {
        let starts = [10.0, 20.0, 30.0];
        assert_eq!(next_cue(starts.into_iter(), 15.0, true), Some(20.0));
        assert_eq!(next_cue(starts.into_iter(), 20.0, true), Some(30.0));
        assert_eq!(next_cue(starts.into_iter(), 20.0, false), Some(10.0));
        assert_eq!(next_cue(starts.into_iter(), 30.0, true), None);
        assert_eq!(next_cue(starts.into_iter(), 5.0, false), None);
    }

    #[test]
    fn tap_tempo_needs_four_taps_and_restarts_after_a_pause() {
        let mut taps = Vec::new();
        assert_eq!(register_tap(&mut taps, 10.0), None);
        assert_eq!(register_tap(&mut taps, 10.5), None);
        assert_eq!(register_tap(&mut taps, 11.0), None);
        assert_eq!(register_tap(&mut taps, 11.5), Some(120.0));
        // Uneven taps average out.
        assert_eq!(register_tap(&mut taps, 11.98), Some(121.21));
        // A pause starts over.
        assert_eq!(register_tap(&mut taps, 20.0), None);
        assert_eq!(taps, vec![20.0]);
        // Never more than 16 taps count.
        let mut many = Vec::new();
        for i in 0..40 {
            register_tap(&mut many, i as f64 * 0.469);
        }
        assert_eq!(many.len(), 16);
        assert!((register_tap(&mut many, 40.0 * 0.469).unwrap() - 127.93).abs() < 0.01);
    }

    #[test]
    fn typed_bpm() {
        assert_eq!(parse_bpm("128"), Some(128.0));
        assert_eq!(parse_bpm(" 127,5 "), Some(127.5));
        assert_eq!(parse_bpm("124.004 BPM"), Some(124.0));
        assert_eq!(parse_bpm("12"), None);
        assert_eq!(parse_bpm("fast"), None);
        assert_eq!(parse_bpm(""), None);
    }

    #[test]
    fn grid_at_bpm_keeps_the_downbeat_and_lock() {
        let mut g = Beatgrid::constant(120.0, 0.25, 3, 60.0);
        g.locked = true;
        let down = g.beats.iter().find(|b| b.bar_position == 1).unwrap().time;
        let t = grid_at_bpm(&g, 125.0, 60.0);
        assert!(t.beats.iter().any(|b| b.bar_position == 1 && (b.time - down).abs() < 1e-9));
        assert!((t.dominant_bpm().unwrap() - 125.0).abs() < 1e-6);
        assert!(t.locked);
        let fresh = grid_at_bpm(&Beatgrid::default(), 100.0, 30.0);
        assert_eq!(fresh.beats[0].time, 0.0);
        assert!((fresh.beats[1].time - 0.6).abs() < 1e-9);
    }

    #[test]
    fn cursor_snaps_near_beats_only() {
        let g = Beatgrid::constant(120.0, 0.0, 1, 60.0);
        assert!((snap_to_beat(&g, 1.03) - 1.0).abs() < 1e-9);
        assert!((snap_to_beat(&g, 1.2) - 1.2).abs() < 1e-9);
    }
}
