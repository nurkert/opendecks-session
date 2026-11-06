//! LIGHTING (SPEC §10.5): feeds the DMX show from what the decks play and
//! drives the LIGHTING view.
//!
//! The show runs on its own thread at 40 frames per second, so DMX timing
//! does not depend on UI repaints. The UI thread only publishes what it
//! knows about the loaded tracks ([`TrackLight`]) and the latest engine
//! state; [`deck_music`] turns both into the show's input. The rig (patch,
//! show settings, outputs) is kept in `lighting.json` next to the library.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use od_core::{MusicalKey, Phrase, PhraseMood};
use od_engine::{DeckState, EngineState, MAX_DECKS};
use od_lighting::{
    DeckMusic, FixtureDef, FixtureRole, LightCue, Lighting, Look, Mode, Momentary, OutputKind, Patch, Patched, Program,
    RigConfig, Show, ShowSettings, Universe, hue_for_key,
};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};
use std::path::PathBuf;
use std::rc::Rc;

use crate::app::{App, handler};
use crate::models::Rows;
use crate::{LightFixtureView, LightGroupView};

/// Frames per second sent to the outputs (DMX refreshes at most ~44 Hz).
pub const FPS: u32 = 40;

/// What the show needs to know about a loaded track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackLight {
    pub phrases: Vec<Phrase>,
    pub mood: Option<PhraseMood>,
    pub key: Option<MusicalKey>,
    /// Energy in 1-second steps, 0..1.
    pub energy: Vec<f32>,
}

/// Show input for one deck (pure).
pub fn deck_music(s: &DeckState, gain_to_master: f32, track: Option<&TrackLight>) -> DeckMusic {
    let beat = s.beat.max(0.0);
    let b = beat as u32;
    let phrase = track.and_then(|t| t.phrases.iter().find(|p| p.start_beat <= b && b < p.end_beat));
    let progress = phrase
        .map(|p| ((beat - p.start_beat as f64) / (p.end_beat - p.start_beat).max(1) as f64) as f32)
        .unwrap_or(0.0);
    let energy = track
        .and_then(|t| t.energy.get(s.position.max(0.0) as usize).copied())
        .unwrap_or(0.5);
    DeckMusic {
        track: s.track.map(|t| t.0),
        playing: s.playing && s.track.is_some(),
        bpm: if s.bpm > 0.0 { s.bpm } else { 120.0 },
        beat,
        phrase: phrase.map(|p| p.kind),
        mood: track.and_then(|t| t.mood),
        phrase_progress: progress.clamp(0.0, 1.0),
        energy,
        weight: gain_to_master,
        hue: track.and_then(|t| t.key).map(|k| hue_for_key(k.camelot_number())).unwrap_or(0.6),
    }
}

/// Show input for all decks.
pub fn music(state: &EngineState, tracks: &[Option<TrackLight>]) -> Vec<DeckMusic> {
    (0..state.decks_count.min(MAX_DECKS))
        .map(|d| deck_music(&state.decks[d], state.channels[d].gain_to_master, tracks.get(d).and_then(Option::as_ref)))
        .collect()
}

/// What the UI reads back for the rig preview and channel monitor.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub looks: Vec<Look>,
    pub universes: Vec<Universe>,
    /// Pro cue in effect.
    pub cue: Option<usize>,
}

enum Msg {
    Engine(Box<EngineState>),
    Track(usize, Option<TrackLight>),
    Patch(Patch),
    Settings(ShowSettings),
    Momentary(Momentary),
    Output(u16, od_lighting::OutputKind),
    ClearOutputs,
    Cue(Option<usize>),
    Stop,
}

/// Handle to the lighting thread.
pub struct Runner {
    tx: Sender<Msg>,
    pub snapshot: Arc<Mutex<Snapshot>>,
}

impl Runner {
    pub fn start(patch: Patch, settings: ShowSettings) -> Self {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let shared = snapshot.clone();
        std::thread::Builder::new()
            .name("od-lighting".into())
            .spawn(move || run(rx, Lighting::new(patch, Show::new(settings)), shared))
            .expect("spawn lighting thread");
        Self { tx, snapshot }
    }

    pub fn engine(&self, state: &EngineState) {
        let _ = self.tx.send(Msg::Engine(Box::new(*state)));
    }

    pub fn track(&self, deck: usize, track: Option<TrackLight>) {
        let _ = self.tx.send(Msg::Track(deck, track));
    }

    pub fn patch(&self, patch: Patch) {
        let _ = self.tx.send(Msg::Patch(patch));
    }

    pub fn settings(&self, settings: ShowSettings) {
        let _ = self.tx.send(Msg::Settings(settings));
    }

    pub fn momentary(&self, m: Momentary) {
        let _ = self.tx.send(Msg::Momentary(m));
    }

    pub fn output(&self, universe: u16, kind: od_lighting::OutputKind) {
        let _ = self.tx.send(Msg::Output(universe, kind));
    }

    pub fn clear_outputs(&self) {
        let _ = self.tx.send(Msg::ClearOutputs);
    }

    /// Selects a Pro cue (Go / Back).
    pub fn cue(&self, cue: Option<usize>) {
        let _ = self.tx.send(Msg::Cue(cue));
    }

    pub fn active_cue(&self) -> Option<usize> {
        self.snapshot.lock().ok().and_then(|s| s.cue)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
    }
}

fn run(rx: Receiver<Msg>, mut rig: Lighting, snapshot: Arc<Mutex<Snapshot>>) {
    let frame = Duration::from_secs(1) / FPS;
    let mut state = EngineState::default();
    let mut tracks: Vec<Option<TrackLight>> = vec![None; MAX_DECKS];
    let mut last = Instant::now();
    let mut next = Instant::now();
    loop {
        let wait = next.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Msg::Engine(s)) => state = *s,
            Ok(Msg::Track(d, t)) => {
                if d < MAX_DECKS {
                    tracks[d] = t;
                }
            }
            Ok(Msg::Patch(p)) => rig.patch = p,
            Ok(Msg::Settings(s)) => rig.show.settings = s,
            Ok(Msg::Momentary(m)) => rig.show.momentary = m,
            Ok(Msg::Output(u, kind)) => {
                if let Err(e) = rig.add_output(u, kind) {
                    tracing::warn!("lighting output: {e}");
                }
            }
            Ok(Msg::ClearOutputs) => rig.clear_outputs(),
            Ok(Msg::Cue(c)) => rig.show.manual_cue = c,
            Ok(Msg::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if now < next {
            continue;
        }
        let dt = now.duration_since(last).as_secs_f32().min(0.25);
        last = now;
        next += frame;
        if next < now {
            next = now + frame;
        }
        rig.tick(&music(&state, &tracks), dt);
        if let Ok(mut s) = snapshot.lock() {
            s.looks.clone_from(&rig.looks);
            s.universes.clone_from(&rig.frame);
            s.cue = rig.show.active_cue();
        }
    }
}

/// Colours offered in Easy and Pro mode.
pub const SWATCHES: [[f32; 3]; 8] = [
    [1.0, 0.1, 0.1],
    [1.0, 0.45, 0.0],
    [1.0, 0.8, 0.1],
    [0.1, 1.0, 0.2],
    [0.0, 0.85, 1.0],
    [0.1, 0.25, 1.0],
    [0.65, 0.1, 1.0],
    [1.0, 1.0, 1.0],
];

fn program_label(p: Program) -> &'static str {
    match p {
        Program::Static => "STATIC",
        Program::Pulse => "PULSE",
        Program::Chase => "CHASE",
        Program::Sweep => "SWEEP",
        Program::Breathe => "BREATHE",
        Program::Build => "BUILD",
        Program::Strobe => "STROBE",
        Program::Blackout => "DARK",
    }
}

fn swatch_index(c: Option<[f32; 3]>) -> i32 {
    c.and_then(|c| SWATCHES.iter().position(|s| *s == c)).map_or(-1, |i| i as i32)
}

fn program_index(p: Option<Program>) -> i32 {
    p.and_then(|p| Program::ALL.iter().position(|x| *x == p)).map_or(-1, |i| i as i32)
}

/// The rig file of a library.
pub fn rig_path(library: &std::path::Path) -> PathBuf {
    library.parent().map(|d| d.join("lighting.json")).unwrap_or_else(|| PathBuf::from("lighting.json"))
}

/// Stage position of each fixture: movers on the top truss, bars and
/// effects in the middle, washes at the front.
pub fn stage_positions(patch: &Patch) -> Vec<(f32, f32)> {
    let row = |r: FixtureRole| match r {
        FixtureRole::Mover => 0,
        FixtureRole::Bar | FixtureRole::Effect => 1,
        FixtureRole::Wash => 2,
    };
    let mut counts = [0usize; 3];
    for f in &patch.fixtures {
        counts[row(f.def.role())] += 1;
    }
    let mut seen = [0usize; 3];
    patch
        .fixtures
        .iter()
        .map(|f| {
            let r = row(f.def.role());
            let i = seen[r];
            seen[r] += 1;
            ((i as f32 + 0.5) / counts[r] as f32, [0.18, 0.5, 0.82][r])
        })
        .collect()
}

fn rgb(c: [f32; 3]) -> Color {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::from_rgb_u8(b(c[0]), b(c[1]), b(c[2]))
}

/// View rows for the stage and rig list (pure).
pub fn fixture_views(patch: &Patch, looks: &[Look]) -> Vec<LightFixtureView> {
    let pos = stage_positions(patch);
    patch
        .fixtures
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let look = looks.get(i).cloned().unwrap_or_default();
            let role = f.def.role();
            let (x, y) = pos[i];
            let pan = if f.invert_pan { 1.0 - look.pan } else { look.pan };
            let cells = f.def.pixels() as usize;
            let cell_colors: Vec<Color> = if role == FixtureRole::Bar {
                (0..cells)
                    .map(|c| {
                        let px = if look.pixels.is_empty() {
                            look.color
                        } else {
                            look.pixels[(c * look.pixels.len()) / cells.max(1)]
                        };
                        rgb(px)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            LightFixtureView {
                name: f.name.clone().into(),
                group: f.group.clone().into(),
                role: match role {
                    FixtureRole::Wash => "wash",
                    FixtureRole::Mover => "mover",
                    FixtureRole::Bar => "bar",
                    FixtureRole::Effect => "effect",
                }
                .into(),
                address: format!("{}·{:03}", f.universe + 1, f.address).into(),
                x,
                y,
                color: rgb(look.color),
                intensity: look.intensity.clamp(0.0, 1.0),
                beam_x: (x + (pan - 0.5) * 0.7).clamp(0.0, 1.0),
                beam_y: (y + 0.25 + 0.5 * look.tilt).clamp(0.0, 1.0),
                strobe: look.strobe > 0.0,
                cells: ModelRc::new(VecModel::from(cell_colors)),
            }
        })
        .collect()
}

/// "DECK 1 · CHORUS → SWEEP": what the show follows now (pure).
pub fn status_text(mode: Mode, decks: &[DeckMusic]) -> String {
    let dominant = decks
        .iter()
        .enumerate()
        .filter(|(_, d)| d.playing && d.weight > 0.01)
        .max_by(|a, b| a.1.weight.total_cmp(&b.1.weight));
    match (mode, dominant) {
        (_, None) => "NO MUSIC · IDLE WASH".into(),
        (Mode::Auto, Some((i, d))) => {
            let phrase = d.phrase.map(|p| crate::render::phrase_label(p).to_string()).unwrap_or_else(|| "—".into());
            let mixing = decks.iter().filter(|d| d.playing && d.weight > 0.01).count() > 1;
            format!(
                "DECK {} · {phrase} → {}{}",
                i + 1,
                program_label(Program::for_phrase(d.phrase)),
                if mixing { " · MIX BLEND" } else { "" }
            )
        }
        (Mode::Easy, Some((i, _))) => format!("EASY · BEAT FROM DECK {}", i + 1),
        (Mode::Pro, Some((i, _))) => format!("PRO · BEAT FROM DECK {}", i + 1),
    }
}

/// UI side of the lighting: the rig file, the runner and the view rows.
pub struct LightingRuntime {
    pub config: RigConfig,
    path: PathBuf,
    runner: Runner,
    momentary: Momentary,
    /// Track sent to the runner per deck.
    sent: [Option<od_core::TrackId>; MAX_DECKS],
    tracks: Vec<Option<TrackLight>>,
    fixtures: Rows<LightFixtureView>,
}

impl LightingRuntime {
    pub fn load(path: PathBuf) -> Self {
        let config = RigConfig::load(&path).unwrap_or_else(|e| {
            tracing::warn!("lighting rig {}: {e}", path.display());
            RigConfig::default()
        });
        let runner = Runner::start(config.patch.clone(), config.settings.clone());
        let rt = Self {
            config,
            path,
            runner,
            momentary: Momentary::default(),
            sent: [None; MAX_DECKS],
            tracks: vec![None; MAX_DECKS],
            fixtures: Rows::default(),
        };
        rt.apply_outputs();
        rt
    }

    fn apply_outputs(&self) {
        self.runner.clear_outputs();
        if self.config.enabled {
            for (u, kind) in &self.config.outputs {
                self.runner.output(*u, kind.clone());
            }
        }
    }

    fn save(&self) {
        if let Err(e) = self.config.save(&self.path) {
            tracing::warn!("saving lighting rig: {e}");
        }
    }

    /// Sends patch and settings to the show and stores the rig.
    fn commit(&self) {
        self.runner.patch(self.config.patch.clone());
        self.runner.settings(self.config.settings.clone());
        self.save();
    }
}

fn refresh(app: &App) {
    let rt = app.lighting.borrow();
    let g = app.ui.global::<crate::Lighting>();
    let s = &rt.config.settings;
    g.set_enabled(rt.config.enabled);
    g.set_mode(match s.mode {
        Mode::Auto => "auto",
        Mode::Easy => "easy",
        Mode::Pro => "pro",
    }
    .into());
    g.set_master(s.master);
    g.set_blackout(s.blackout);
    g.set_flash(rt.momentary.flash);
    g.set_strobe(rt.momentary.strobe);
    g.set_easy_program(program_index(Some(s.easy_program)));
    g.set_easy_color(swatch_index(s.easy_color));
    let groups: Vec<LightGroupView> = rt
        .config
        .patch
        .groups()
        .into_iter()
        .map(|name| {
            let gs = s.groups.get(&name).cloned().unwrap_or_default();
            LightGroupView { name: name.into(), program: program_index(gs.program), color: swatch_index(gs.color) }
        })
        .collect();
    g.set_groups(ModelRc::new(VecModel::from(groups)));
    let cues: Vec<SharedString> = s.cues.iter().map(|c| SharedString::from(c.name.as_str())).collect();
    g.set_cues(ModelRc::new(VecModel::from(cues)));
    let outputs: Vec<String> = rt
        .config
        .outputs
        .iter()
        .map(|(u, k)| {
            let to = match k {
                OutputKind::ArtNet { target, .. } => format!("Art-Net {target}"),
                OutputKind::Sacn { universe } => format!("sACN universe {universe}"),
                OutputKind::EnttecPro { device } => format!("USB-DMX {device}"),
            };
            format!("U{} → {to}", u + 1)
        })
        .collect();
    g.set_output_text(if outputs.is_empty() { "No output set".into() } else { outputs.join(" · ").into() });
    g.set_rig_path(rt.path.display().to_string().into());
    // Rows also when no frame arrives (engine stopped): the rig list.
    let looks = rt.runner.snapshot.lock().map(|s| s.looks.clone()).unwrap_or_default();
    rt.fixtures.sync(fixture_views(&rt.config.patch, &looks), |_, _| false);
}

fn edit(app: &App, f: impl FnOnce(&mut LightingRuntime)) {
    {
        let mut rt = app.lighting.borrow_mut();
        f(&mut rt);
        rt.commit();
    }
    refresh(app);
}

pub(crate) fn wire(app: &Rc<App>) {
    let g = app.ui.global::<crate::Lighting>();
    g.set_fixtures(app.lighting.borrow().fixtures.handle());
    g.set_programs(ModelRc::new(VecModel::from(
        Program::ALL.iter().map(|p| SharedString::from(program_label(*p))).collect::<Vec<_>>(),
    )));
    g.set_swatches(ModelRc::new(VecModel::from(SWATCHES.iter().map(|c| rgb(*c)).collect::<Vec<_>>())));
    g.set_fixture_types(ModelRc::new(VecModel::from(
        FixtureDef::BUILTIN
            .iter()
            .map(|n| SharedString::from(FixtureDef::builtin(n).map(|d| d.model).unwrap_or_default()))
            .collect::<Vec<_>>(),
    )));
    g.on_set_enabled(handler!(app, |a, on: bool| {
        {
            let mut rt = a.lighting.borrow_mut();
            rt.config.enabled = on;
            rt.apply_outputs();
            rt.save();
        }
        refresh(&a)
    }));
    g.on_set_mode(handler!(app, |a, m: SharedString| edit(&a, |rt| {
        rt.config.settings.mode = match m.as_str() {
            "easy" => Mode::Easy,
            "pro" => Mode::Pro,
            _ => Mode::Auto,
        }
    })));
    g.on_set_master(handler!(app, |a, v: f32| edit(&a, |rt| rt.config.settings.master = v.clamp(0.0, 1.0))));
    g.on_set_blackout(handler!(app, |a, on: bool| edit(&a, |rt| rt.config.settings.blackout = on)));
    g.on_flash_held(handler!(app, |a, on: bool| momentary(&a, |m| m.flash = on)));
    g.on_strobe_held(handler!(app, |a, on: bool| momentary(&a, |m| m.strobe = on)));
    g.on_choose_easy_program(handler!(app, |a, i: i32| edit(&a, |rt| {
        if let Some(p) = Program::ALL.get(i as usize) {
            rt.config.settings.easy_program = *p;
        }
    })));
    g.on_choose_easy_color(handler!(app, |a, i: i32| edit(&a, |rt| rt.config.settings.easy_color = SWATCHES.get(i as usize).copied())));
    g.on_group_program(handler!(app, |a, gi: i32, pi: i32| edit(&a, |rt| {
        if let Some(name) = rt.config.patch.groups().get(gi as usize) {
            rt.config.settings.groups.entry(name.clone()).or_default().program = Program::ALL.get(pi as usize).copied();
        }
    })));
    g.on_group_color(handler!(app, |a, gi: i32, ci: i32| edit(&a, |rt| {
        if let Some(name) = rt.config.patch.groups().get(gi as usize) {
            rt.config.settings.groups.entry(name.clone()).or_default().color = SWATCHES.get(ci as usize).copied();
        }
    })));
    g.on_add_fixture(handler!(app, |a, ti: i32, name: SharedString, group: SharedString| {
        let result = add_fixture(&mut a.lighting.borrow_mut().config.patch, ti as usize, name.as_str(), group.as_str());
        match result {
            Ok(_) => edit(&a, |_| {}),
            Err(e) => a.status(format!("Lighting: {e}")),
        }
    }));
    g.on_remove_fixture(handler!(app, |a, i: i32| edit(&a, |rt| {
        if (i as usize) < rt.config.patch.fixtures.len() {
            rt.config.patch.fixtures.remove(i as usize);
        }
    })));
    g.on_set_output(handler!(app, |a, kind: SharedString, target: SharedString| {
        match parse_output(kind.as_str(), target.as_str()) {
            Ok(k) => {
                {
                    let mut rt = a.lighting.borrow_mut();
                    rt.config.outputs.retain(|(u, _)| *u != 0);
                    rt.config.outputs.push((0, k));
                    rt.config.enabled = true;
                    rt.apply_outputs();
                    rt.save();
                }
                refresh(&a);
            }
            Err(e) => a.status(format!("Lighting: {e}")),
        }
    }));
    g.on_cue_go(handler!(app, |a| cue_step(&a, true)));
    g.on_cue_back(handler!(app, |a| cue_step(&a, false)));
    g.on_cue_store(handler!(app, |a, name: SharedString| edit(&a, |rt| {
        let s = &mut rt.config.settings;
        let name = if name.trim().is_empty() { format!("Cue {}", s.cues.len() + 1) } else { name.to_string() };
        s.cues.push(LightCue { name, groups: s.groups.clone(), fade_beats: 4.0 });
    })));
    g.on_cue_delete(handler!(app, |a, i: i32| edit(&a, |rt| {
        if (i as usize) < rt.config.settings.cues.len() {
            rt.config.settings.cues.remove(i as usize);
        }
    })));
    refresh(app);
}

fn momentary(app: &App, f: impl FnOnce(&mut Momentary)) {
    {
        let mut rt = app.lighting.borrow_mut();
        f(&mut rt.momentary);
        rt.runner.momentary(rt.momentary);
    }
    refresh(app);
}

fn cue_step(app: &App, forward: bool) {
    let (n, active) = {
        let rt = app.lighting.borrow();
        (rt.config.settings.cues.len() as i32, app.ui.global::<crate::Lighting>().get_active_cue())
    };
    if n == 0 {
        return;
    }
    let next = if active < 0 { if forward { 0 } else { n - 1 } } else if forward { (active + 1) % n } else { (active + n - 1) % n };
    app.ui.global::<crate::Lighting>().set_active_cue(next);
    app.lighting.borrow().runner.cue(Some(next as usize));
}

/// Patches a built-in fixture type at the next free address (pure on the patch).
pub fn add_fixture(patch: &mut Patch, type_index: usize, name: &str, group: &str) -> Result<usize, od_lighting::Error> {
    let kind = FixtureDef::BUILTIN.get(type_index).ok_or_else(|| od_lighting::Error::Fixture("unknown type".into()))?;
    let def = FixtureDef::builtin(kind).expect("built-in");
    let len = def.channels.len() as u16;
    let (universe, address) = (0..16u16)
        .find_map(|u| patch.next_free(u, len).map(|a| (u, a)))
        .ok_or_else(|| od_lighting::Error::Patch("no free DMX address".into()))?;
    let group = if group.trim().is_empty() {
        match def.role() {
            FixtureRole::Mover => "Movers",
            FixtureRole::Bar => "Bars",
            FixtureRole::Wash => "Wash",
            FixtureRole::Effect => "Effects",
        }
        .to_string()
    } else {
        group.trim().to_string()
    };
    let name = if name.trim().is_empty() {
        format!("{} {}", def.model, patch.fixtures.iter().filter(|f| f.def.model == def.model).count() + 1)
    } else {
        name.trim().to_string()
    };
    patch.add(Patched { name, def, universe, address, group, invert_pan: false })
}

/// Output from the view's kind and target text (pure).
pub fn parse_output(kind: &str, target: &str) -> Result<OutputKind, String> {
    let target = target.trim();
    match kind {
        "artnet" => {
            let t = if target.is_empty() { "255.255.255.255" } else { target };
            let t = if t.contains(':') { t.to_string() } else { format!("{t}:6454") };
            t.parse().map(|target| OutputKind::ArtNet { target, universe: 0 }).map_err(|_| format!("not an address: {t}"))
        }
        "sacn" => {
            let u = if target.is_empty() { 1 } else { target.parse().map_err(|_| format!("not a universe: {target}"))? };
            if !(1..=63999).contains(&u) {
                return Err(format!("sACN universes are 1 to 63999, not {u}"));
            }
            Ok(OutputKind::Sacn { universe: u })
        }
        "enttec" if !target.is_empty() => Ok(OutputKind::EnttecPro { device: target.to_string() }),
        "enttec" => Err("enter the serial device of the USB-DMX interface".into()),
        _ => Err(format!("unknown output {kind}")),
    }
}

/// Per engine frame: feeds the show and, in LIGHTING mode, the preview.
pub(crate) fn frame(app: &App, state: Option<&EngineState>) {
    let Some(state) = state else {
        if app.ui.get_mode() == "lighting" {
            refresh_stage(app, &[]);
        }
        return;
    };
    // Track metadata when a deck's track changes.
    let changed: Vec<(usize, Option<od_core::TrackId>)> = {
        let rt = app.lighting.borrow();
        (0..MAX_DECKS).filter(|&d| rt.sent[d] != state.decks[d].track).map(|d| (d, state.decks[d].track)).collect()
    };
    for (d, id) in changed {
        let light = id.and_then(|id| {
            let st = app.state.borrow();
            let analysis = st.src().analysis(id).ok()?;
            let key = st.src().track(id).ok().and_then(|t| t.key).or(analysis.key);
            Some(TrackLight { phrases: analysis.phrases, mood: Some(analysis.phrase_mood), key, energy: analysis.energy })
        });
        let mut rt = app.lighting.borrow_mut();
        rt.sent[d] = id;
        rt.tracks[d].clone_from(&light);
        rt.runner.track(d, light);
    }
    app.lighting.borrow().runner.engine(state);
    let visible = app.ui.get_mode() == "lighting" || (app.ui.get_mode() == "perform" && app.ui.global::<crate::Lighting>().get_panel());
    if visible {
        let decks = music(state, &app.lighting.borrow().tracks);
        refresh_stage(app, &decks);
    }
}

fn refresh_stage(app: &App, decks: &[DeckMusic]) {
    let rt = app.lighting.borrow();
    let looks = rt.runner.snapshot.lock().map(|s| s.looks.clone()).unwrap_or_default();
    rt.fixtures.sync(fixture_views(&rt.config.patch, &looks), |a, b| {
        a.color == b.color && a.intensity == b.intensity && a.beam_x == b.beam_x && a.beam_y == b.beam_y && a.strobe == b.strobe
            && a.name == b.name && a.role != "bar"
    });
    let g = app.ui.global::<crate::Lighting>();
    g.set_status(status_text(rt.config.settings.mode, decks).into());
    if let Some(c) = rt.runner.active_cue() {
        g.set_active_cue(c as i32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{PhraseKind, TrackId};
    use slint::Model;

    fn track() -> TrackLight {
        TrackLight {
            phrases: vec![
                Phrase { start_beat: 0, end_beat: 64, kind: PhraseKind::Intro },
                Phrase { start_beat: 64, end_beat: 128, kind: PhraseKind::Chorus },
            ],
            mood: Some(PhraseMood::High),
            key: None,
            energy: vec![0.2, 0.9],
        }
    }

    #[test]
    fn deck_music_reads_phrase_progress_and_energy() {
        let s = DeckState { track: Some(TrackId(1)), playing: true, beat: 80.0, bpm: 126.0, position: 1.5, ..Default::default() };
        let m = deck_music(&s, 0.7, Some(&track()));
        assert_eq!(m.phrase, Some(PhraseKind::Chorus));
        assert!((m.phrase_progress - 0.25).abs() < 1e-6);
        assert_eq!((m.energy, m.weight, m.bpm), (0.9, 0.7, 126.0));
        assert!(m.playing);
        let empty = deck_music(&DeckState::default(), 0.0, None);
        assert!(!empty.playing);
        assert_eq!((empty.phrase, empty.bpm), (None, 120.0));
    }

    #[test]
    fn add_fixture_picks_address_group_and_name() {
        let mut patch = Patch::default();
        let mover = FixtureDef::BUILTIN.iter().position(|n| *n == "moving-head").unwrap();
        add_fixture(&mut patch, mover, "", "").unwrap();
        add_fixture(&mut patch, mover, "Spot R", "Truss").unwrap();
        assert_eq!(patch.fixtures[0].name, "Moving head 1");
        assert_eq!(patch.fixtures[0].group, "Movers");
        assert_eq!((patch.fixtures[1].address, patch.fixtures[1].group.as_str()), (15, "Truss"));
        assert!(add_fixture(&mut patch, 99, "", "").is_err());
    }

    #[test]
    fn output_text_is_parsed() {
        assert_eq!(
            parse_output("artnet", "10.0.0.5").unwrap(),
            OutputKind::ArtNet { target: "10.0.0.5:6454".parse().unwrap(), universe: 0 }
        );
        assert_eq!(parse_output("sacn", "").unwrap(), OutputKind::Sacn { universe: 1 });
        assert!(parse_output("sacn", "70000").is_err());
        assert!(parse_output("enttec", "").is_err());
        assert!(parse_output("artnet", "nonsense").is_err());
    }

    #[test]
    fn stage_and_status() {
        let mut patch = Patch::default();
        for t in ["moving-head", "rgbw-par", "rgbw-par", "light-bar-8"] {
            let i = FixtureDef::BUILTIN.iter().position(|n| *n == t).unwrap();
            add_fixture(&mut patch, i, "", "").unwrap();
        }
        let pos = stage_positions(&patch);
        assert_eq!(pos[0], (0.5, 0.18));
        assert_eq!((pos[1], pos[2]), ((0.25, 0.82), (0.75, 0.82)));
        let looks = vec![Look { intensity: 1.0, color: [1.0, 0.0, 0.0], ..Look::default() }; 4];
        let views = fixture_views(&patch, &looks);
        assert_eq!(views[0].role, "mover");
        assert_eq!(views[3].cells.row_count(), 8);
        assert_eq!(views[1].color, Color::from_rgb_u8(255, 0, 0));
        let deck = DeckMusic { playing: true, weight: 1.0, phrase: Some(od_core::PhraseKind::Chorus), ..Default::default() };
        assert!(status_text(Mode::Auto, &[deck]).ends_with("→ SWEEP"));
        assert!(status_text(Mode::Auto, &[deck, deck]).contains("MIX BLEND"));
        assert_eq!(status_text(Mode::Auto, &[]), "NO MUSIC · IDLE WASH");
    }

    #[test]
    fn runner_renders_frames_on_its_own_thread() {
        let mut patch = Patch::default();
        patch
            .add(od_lighting::Patched {
                name: "Par".into(),
                def: od_lighting::FixtureDef::builtin("rgb-par").unwrap(),
                universe: 0,
                address: 1,
                group: "Wash".into(),
                invert_pan: false,
            })
            .unwrap();
        let runner = Runner::start(patch, ShowSettings::default());
        let mut state = EngineState::default();
        state.decks[0] = DeckState { track: Some(TrackId(1)), playing: true, beat: 70.0, bpm: 128.0, ..Default::default() };
        state.channels[0].gain_to_master = 1.0;
        runner.track(0, Some(track()));
        runner.engine(&state);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let lit = runner.snapshot.lock().unwrap().universes.first().is_some_and(|u| u[..3].iter().any(|&v| v > 100));
            if lit {
                break;
            }
            assert!(Instant::now() < deadline, "no chorus frame rendered");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
