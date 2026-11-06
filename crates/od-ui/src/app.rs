//! The Prepare-side application controller: owns the library connection on
//! the UI thread, keeps the view models in sync and handles callbacks.

use crate::workers::{AnalysisWorker, WorkerEvent, spawn_import};
use crate::{AppWindow, CueMark, DeviceRow, FlowPoint, HotCueSlot, NodeChoice, ProposalRow, RecordingRow, TrackRow, TreeRow, VolumeRow};
use crate::{render, smart_text};
use crossbeam_channel::{Receiver, unbounded};
use od_core::{Beatgrid, Cue, CueKind, DeviceId, KeyNotation, TrackId};
use od_devices::{Command, DeviceManager, DeviceState, DeviceStatus, Event, ManagerConfig, Proposal, Volume};
use od_library::sets::Fit;
use od_library::{DeviceFormats, Library, NodeId, NodeKind, SortKey, TrackRecord};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Settings the app starts with.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub library_path: PathBuf,
    /// Raspberry Pi / weak hardware.
    pub lite: bool,
    pub kiosk: bool,
    pub key_notation: KeyNotation,
    /// "de" selects German; anything else English.
    pub language: String,
    /// Fixed window size in logical pixels (e.g. to emulate a Pi display);
    /// disables full screen in kiosk mode.
    pub window: Option<(u32, u32)>,
    pub colorblind_safe: bool,
    pub high_contrast: bool,
    /// Animations; always off in the Lite profile.
    pub motion: bool,
}

pub(crate) struct State {
    pub lib: Library,
    pub config: AppConfig,
    node: Option<NodeId>,
    sort: String,
    descending: bool,
    search: String,
    rows: Vec<TrackRecord>,
    selection: BTreeSet<usize>,
    anchor: Option<usize>,
    pub current: Option<TrackId>,
    /// Cursor in the selected track, seconds.
    pub cursor: f64,
    expanded: HashSet<NodeId>,
    device_selected: Option<DeviceId>,
    statuses: Vec<DeviceStatus>,
    volumes: Vec<Volume>,
    proposals: Vec<(DeviceId, Proposal)>,
    flow_selected: Option<usize>,
    last_seq: i64,
    analysis: (usize, usize),
    /// Mounted sticks carrying an OpenDecks library: (name, root).
    sticks: Vec<(String, PathBuf)>,
    /// The stick being browsed (read-only source), with its index in `sticks`.
    stick: Option<(usize, od_format_native::Stick)>,
}

/// Tree ids: stick roots are `STICK_ROOT - i`, nodes inside stick `i` are
/// `-(STICK_NODE * (i + 1) + node)`.
const STICK_ROOT: i32 = -100;
const STICK_NODE: i32 = 1_000_000;

fn decode_stick_id(id: i32) -> Option<(usize, Option<NodeId>)> {
    if id <= -STICK_NODE {
        let v = -id;
        Some(((v / STICK_NODE - 1) as usize, Some(NodeId((v % STICK_NODE) as i64))))
    } else if id <= STICK_ROOT && id > STICK_ROOT - 1000 {
        Some(((STICK_ROOT - id) as usize, None))
    } else {
        None
    }
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

fn fmt_cue_time(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn fmt_bytes(b: u64) -> String {
    let gb = b as f64 / 1e9;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", b as f64 / 1e6) }
}

fn sort_key(s: &str) -> Option<SortKey> {
    Some(match s {
        "date_added" => SortKey::DateAdded,
        "title" => SortKey::Title,
        "artist" => SortKey::Artist,
        "genre" => SortKey::Genre,
        "bpm" => SortKey::Bpm,
        "key" => SortKey::Key,
        "rating" => SortKey::Rating,
        "duration" => SortKey::Duration,
        _ => return None,
    })
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

impl State {
    /// The library the current view reads from: a browsed stick or the main
    /// library.
    pub(crate) fn src(&self) -> &Library {
        self.stick.as_ref().map(|(_, s)| &s.library).unwrap_or(&self.lib)
    }

    /// Absolute path of a track of the current source.
    pub(crate) fn resolve(&self, path: &std::path::Path) -> PathBuf {
        match &self.stick {
            Some((_, s)) => s.resolve(path),
            None => path.to_path_buf(),
        }
    }

    pub(crate) fn read_only(&self) -> bool {
        self.stick.is_some()
    }

    fn node_kind(&self) -> Option<NodeKind> {
        self.node.and_then(|n| self.src().node(n).ok()).map(|n| n.kind)
    }

    fn key_text(&self, t: &TrackRecord) -> String {
        t.key.map(|k| k.format(self.config.key_notation)).unwrap_or_default()
    }

    fn selected_ids(&self) -> Vec<TrackId> {
        self.selection.iter().filter_map(|&i| self.rows.get(i)).map(|t| t.id).collect()
    }

    fn load_rows(&mut self) {
        let mut rows = match self.node {
            None if !self.search.trim().is_empty() => self.src().search(&self.search, 5000).unwrap_or_default(),
            None => self.src().tracks(sort_key(&self.sort).unwrap_or_default(), self.descending).unwrap_or_default(),
            Some(n) => {
                let mut r = self.src().node_tracks(n).unwrap_or_default();
                if !self.search.trim().is_empty() {
                    let q = self.search.to_lowercase();
                    r.retain(|t| {
                        format!("{} {} {} {}", t.tags.title, t.tags.artist, t.tags.album, t.tags.genre).to_lowercase().contains(&q)
                    });
                }
                r
            }
        };
        if self.node.is_some() || !self.search.trim().is_empty() {
            if let Some(k) = sort_key(&self.sort) {
                rows.sort_by(|a, b| {
                    let o = match k {
                        SortKey::Title => a.tags.title.to_lowercase().cmp(&b.tags.title.to_lowercase()),
                        SortKey::Artist => a.tags.artist.to_lowercase().cmp(&b.tags.artist.to_lowercase()),
                        SortKey::Genre => a.tags.genre.to_lowercase().cmp(&b.tags.genre.to_lowercase()),
                        SortKey::Bpm => a.bpm.unwrap_or(0.0).total_cmp(&b.bpm.unwrap_or(0.0)),
                        SortKey::Key => a.key.map(|k| (k.camelot_number(), k.mode as u8)).cmp(&b.key.map(|k| (k.camelot_number(), k.mode as u8))),
                        SortKey::Rating => a.tags.rating.cmp(&b.tags.rating),
                        SortKey::Duration => a.duration.total_cmp(&b.duration),
                        _ => a.date_added.cmp(&b.date_added),
                    };
                    if self.descending { o.reverse() } else { o }
                });
            }
        }
        // Keep the selection on the same tracks.
        let selected: HashSet<TrackId> = self.selected_ids().into_iter().collect();
        self.rows = rows;
        self.selection = self.rows.iter().enumerate().filter(|(_, t)| selected.contains(&t.id)).map(|(i, _)| i).collect();
    }
}

pub struct App {
    pub ui: AppWindow,
    pub(crate) state: Rc<RefCell<State>>,
    pub(crate) perform: RefCell<crate::perform::Perform>,
    devices: Rc<DeviceManager>,
    pub(crate) analysis: Rc<AnalysisWorker>,
    worker_events: Receiver<WorkerEvent>,
    _timer: slint::Timer,
}

impl App {
    pub fn new(config: AppConfig) -> anyhow::Result<Rc<Self>> {
        let lib = Library::open(&config.library_path)?;
        if lib.backups().map(|b| b.is_empty()).unwrap_or(true) || lib.track_count()? > 0 {
            if let Err(e) = lib.backup_now("start") {
                tracing::warn!(error = %e, "start-up backup failed");
            }
        }
        let ui = AppWindow::new()?;
        if config.language.starts_with("de") {
            if let Err(e) = slint::select_bundled_translation("de") {
                tracing::warn!(error = %e, "German translation unavailable");
            }
            od_i18n::set_language(od_i18n::Language::German);
        }
        ui.set_kiosk(config.kiosk);
        {
            let theme = ui.global::<crate::Theme>();
            theme.set_colorblind_safe(config.colorblind_safe);
            theme.set_high_contrast(config.high_contrast);
            theme.set_motion(config.motion && !config.lite);
            crate::render::set_colorblind_safe(config.colorblind_safe);
        }
        if config.kiosk {
            ui.global::<crate::Theme>().set_row_height(44.0);
        }
        ui.set_ground_texture(crate::render::ground_texture());
        if config.kiosk {
            ui.set_mode("perform".into());
            // Nothing is loaded yet: start on the browse screen.
            ui.set_browser_open(true);
        }
        match config.window {
            Some((w, h)) => {
                let size = slint::LogicalSize::new(w as f32, h as f32);
                ui.window().set_size(size);
                // The first layout pass may resize the window; apply again once shown.
                let weak = ui.as_weak();
                slint::Timer::single_shot(std::time::Duration::from_millis(150), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.window().set_size(size);
                    }
                });
            }
            None if config.kiosk => ui.window().set_fullscreen(true),
            None => {}
        }
        let (wtx, wrx) = unbounded();
        let analysis = Rc::new(AnalysisWorker::start(config.library_path.clone(), config.lite, wtx.clone()));
        let devices = Rc::new(DeviceManager::start(ManagerConfig::new(config.library_path.clone()))?);
        let state = Rc::new(RefCell::new(State {
            lib,
            config,
            node: None,
            sort: "date_added".into(),
            descending: true,
            search: String::new(),
            rows: Vec::new(),
            selection: BTreeSet::new(),
            anchor: None,
            current: None,
            cursor: 0.0,
            expanded: HashSet::new(),
            device_selected: None,
            statuses: Vec::new(),
            volumes: Vec::new(),
            proposals: Vec::new(),
            flow_selected: None,
            last_seq: -1,
            analysis: (0, 0),
            sticks: Vec::new(),
            stick: None,
        }));
        let app = Rc::new(App {
            ui,
            state,
            perform: RefCell::new(Default::default()),
            devices,
            analysis,
            worker_events: wrx,
            _timer: slint::Timer::default(),
        });
        app.wire(wtx);
        app.restore_view();
        app.refresh_all();
        let weak = Rc::downgrade(&app);
        app._timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            if let Some(app) = weak.upgrade() {
                app.tick();
            }
        });
        Ok(app)
    }

    /// Restores the last list, track, section and mode.
    fn restore_view(&self) {
        let mut st = self.state.borrow_mut();
        let get = |k: &str| st.lib.meta(k).ok().flatten();
        let node = get("ui.node").and_then(|v| v.parse::<i64>().ok()).map(NodeId);
        let track = get("ui.track").and_then(|v| v.parse::<i64>().ok()).map(TrackId);
        let section = get("ui.section");
        let mode = get("ui.mode");
        if let Some(n) = node.filter(|n| st.lib.node(*n).is_ok()) {
            st.node = Some(n);
            let in_list = st.lib.node(n).is_ok_and(|n| n.kind.has_entries());
            st.sort = if in_list { "position".into() } else { "date_added".into() };
            st.descending = !in_list;
            let mut p = st.lib.node(n).ok().and_then(|n| n.parent);
            while let Some(x) = p {
                st.expanded.insert(x);
                p = st.lib.node(x).ok().and_then(|n| n.parent);
            }
        }
        st.load_rows();
        if let Some(t) = track {
            if let Some(i) = st.rows.iter().position(|r| r.id == t) {
                st.selection = [i].into_iter().collect();
                st.anchor = Some(i);
                st.current = Some(t);
                if st.node_kind() == Some(NodeKind::Set) {
                    st.flow_selected = Some(i);
                }
            }
        }
        st.device_selected = st.lib.devices().ok().and_then(|d| d.first().map(|d| d.id));
        drop(st);
        if let Some(s) = section {
            self.ui.set_section(s.into());
        }
        if let Some(m) = mode.filter(|_| !self.state.borrow().config.kiosk) {
            self.ui.set_mode(m.into());
        }
    }

    /// Remembers the view for the next start (best effort).
    pub(crate) fn remember_view(&self) {
        let mut st = self.state.borrow_mut();
        let node = st.node.map(|n| n.0.to_string()).unwrap_or_else(|| "-1".into());
        let track = st.current.map(|t| t.0.to_string()).unwrap_or_default();
        let _ = st.lib.set_meta("ui.node", &node);
        let _ = st.lib.set_meta("ui.track", &track);
        let _ = st.lib.set_meta("ui.section", self.ui.get_section().as_str());
        let _ = st.lib.set_meta("ui.mode", self.ui.get_mode().as_str());
    }

    pub fn run(self: &Rc<Self>) -> anyhow::Result<()> {
        self.ui.run()?;
        self.remember_view();
        Ok(())
    }

    // ── Refresh ─────────────────────────────────────────────────────────

    pub fn refresh_all(&self) {
        self.state.borrow_mut().load_rows();
        self.refresh_tree();
        self.refresh_tracks();
        self.refresh_detail();
        self.refresh_devices();
        self.refresh_status();
        self.refresh_recordings();
    }

    fn refresh_tree(&self) {
        let st = self.state.borrow();
        let all = st.lib.all_nodes().unwrap_or_default();
        let device_nodes: HashSet<NodeId> = st
            .lib
            .devices()
            .unwrap_or_default()
            .iter()
            .flat_map(|d| st.lib.device_node_closure(d.id).unwrap_or_default())
            .collect();
        let mut rows = vec![TreeRow {
            id: -1,
            name: od_i18n::tr("All tracks").into(),
            kind: "all".into(),
            depth: 0,
            count: st.lib.track_count().unwrap_or(0) as i32,
            length_frac: 0.0,
            duration: SharedString::new(),
            has_children: false,
            expanded: false,
            on_device: false,
        }];
        let mut durations = Vec::new();
        fn walk(
            st: &State,
            all: &[od_library::Node],
            parent: Option<NodeId>,
            depth: i32,
            out: &mut Vec<TreeRow>,
            durations: &mut Vec<f64>,
            device_nodes: &HashSet<NodeId>,
        ) {
            for n in all.iter().filter(|n| n.parent == parent) {
                let has_children = all.iter().any(|c| c.parent == Some(n.id));
                let (count, duration) = if n.kind == NodeKind::Folder {
                    (0, 0.0)
                } else {
                    let t = st.lib.node_tracks(n.id).unwrap_or_default();
                    (t.len(), t.iter().map(|t| t.duration).sum())
                };
                durations.push(duration);
                out.push(TreeRow {
                    id: n.id.0 as i32,
                    name: n.name.clone().into(),
                    kind: n.kind.as_str().into(),
                    depth,
                    count: count as i32,
                    length_frac: duration as f32,
                    duration: fmt_time(duration).into(),
                    has_children,
                    expanded: st.expanded.contains(&n.id),
                    on_device: device_nodes.contains(&n.id),
                });
                if has_children && st.expanded.contains(&n.id) {
                    walk(st, all, Some(n.id), depth + 1, out, durations, device_nodes);
                }
            }
        }
        walk(&st, &all, None, 0, &mut rows, &mut durations, &device_nodes);
        // Sticks with an OpenDecks library, browsable as read-only sources.
        for (i, (name, _)) in st.sticks.iter().enumerate() {
            let open = st.stick.as_ref().filter(|(j, _)| *j == i).map(|(_, s)| s);
            rows.push(TreeRow {
                id: STICK_ROOT - i as i32,
                name: name.clone().into(),
                kind: "stick".into(),
                depth: 0,
                count: match open {
                    Some(s) => s.library.track_count().unwrap_or(0) as i32,
                    None => od_format_native::Stick::open(&st.sticks[i].1)
                        .ok()
                        .flatten()
                        .and_then(|s| s.library.track_count().ok())
                        .unwrap_or(0) as i32,
                },
                length_frac: 0.0,
                duration: SharedString::new(),
                has_children: false,
                expanded: open.is_some(),
                on_device: true,
            });
            if let Some(s) = open {
                for n in s.library.all_nodes().unwrap_or_default() {
                    let depth = {
                        let mut d = 1;
                        let mut p = n.parent;
                        while let Some(x) = p {
                            d += 1;
                            p = s.library.node(x).ok().and_then(|n| n.parent);
                        }
                        d
                    };
                    rows.push(TreeRow {
                        id: -(STICK_NODE * (i as i32 + 1) + n.id.0 as i32),
                        name: n.name.clone().into(),
                        kind: n.kind.as_str().into(),
                        depth,
                        count: s.library.node_tracks(n.id).map(|t| t.len()).unwrap_or(0) as i32,
                        length_frac: 0.0,
                        duration: SharedString::new(),
                        has_children: false,
                        expanded: false,
                        on_device: false,
                    });
                }
            }
        }
        let max = durations.iter().cloned().fold(1.0, f64::max) as f32;
        for r in rows.iter_mut().skip(1) {
            r.length_frac /= max;
        }
        self.ui.set_tree(model(rows));
        let selected = match (&st.stick, st.node) {
            (Some((i, _)), Some(n)) => -(STICK_NODE * (*i as i32 + 1) + n.0 as i32),
            (Some((i, _)), None) => STICK_ROOT - *i as i32,
            (None, n) => n.map(|n| n.0 as i32).unwrap_or(-1),
        };
        self.ui.set_selected_node(selected);
    }

    pub(crate) fn refresh_tracks(&self) {
        let st = self.state.borrow();
        let current_key = st.current.and_then(|id| st.rows.iter().find(|t| t.id == id)).and_then(|t| t.key);
        let kind = st.node_kind();
        let loaded = if st.read_only() == self.perform.borrow().source_is_stick() { self.perform.borrow().loaded_decks() } else { Vec::new() };
        let entries = match (st.node, kind) {
            (Some(n), Some(NodeKind::Set)) => st.src().entries(n).unwrap_or_default(),
            _ => vec![],
        };
        let rows: Vec<TrackRow> = st
            .rows
            .iter()
            .enumerate()
            .map(|(i, t)| TrackRow {
                id: t.id.0 as i32,
                position: i as i32 + 1,
                title: t.tags.title.clone().into(),
                artist: t.tags.artist.clone().into(),
                album: t.tags.album.clone().into(),
                genre: if kind == Some(NodeKind::Set) {
                    entries.get(i).map(|e| e.note.clone()).unwrap_or_default().into()
                } else {
                    t.tags.genre.clone().into()
                },
                bpm: t.bpm.map(|b| format!("{b:.1}")).unwrap_or_default().into(),
                key: st.key_text(t).into(),
                key_distance: match (current_key, t.key) {
                    (Some(a), Some(b)) => a.camelot_distance(b) as i32,
                    _ => -1,
                },
                time: fmt_time(t.duration).into(),
                rating: t.tags.rating as i32,
                analysed: t.analysis_version >= od_analysis::ANALYSIS_VERSION,
                missing: t.missing,
                selected: st.selection.contains(&i),
                note: SharedString::new(),
                deck: loaded.iter().find(|(id, _)| *id == t.id).map(|(_, d)| *d as i32 + 1).unwrap_or(0),
            })
            .collect();
        let total: f64 = st.rows.iter().map(|t| t.duration).sum();
        self.ui.set_tracks(model(rows));
        self.ui.set_sort_column(st.sort.clone().into());
        self.ui.set_sort_descending(st.descending);
        self.ui.set_has_selection(!st.selection.is_empty());
        self.ui.set_selection_count(st.selection.len() as i32);
        let (title, kind_str) = match (st.node.and_then(|n| st.src().node(n).ok()), &st.stick) {
            (Some(n), _) => (n.name.clone(), n.kind.as_str()),
            (None, Some((i, _))) => (st.sticks.get(*i).map(|s| s.0.clone()).unwrap_or_default(), "stick"),
            (None, None) => (od_i18n::tr("All tracks").to_string(), "all"),
        };
        self.ui.set_node_title(title.clone().into());
        self.ui.set_node_kind(kind_str.into());
        self.ui.set_node_sub(od_i18n::trf("{} TRACKS  ·  {}", &[&st.rows.len(), &fmt_time(total)]).into());
        self.ui.set_read_only(st.read_only());
        self.ui.set_register_target(if kind_str == "all" || st.read_only() { SharedString::new() } else { title.to_uppercase().into() });
        if kind == Some(NodeKind::Smart) {
            let rule = st.node.and_then(|n| st.src().node(n).ok()).and_then(|n| n.smart_rule);
            self.ui.set_smart_rule_text(rule.map(|r| smart_text::format(&r)).unwrap_or_default().into());
        }
        // Set flow.
        if let (Some(n), Some(NodeKind::Set)) = (st.node, kind) {
            if let Ok(flow) = st.src().set_flow(n) {
                let fit = |f: Fit| match f {
                    Fit::Good => "good",
                    Fit::Ok => "ok",
                    Fit::Clash => "clash",
                    Fit::Unknown => "unknown",
                };
                let pts: Vec<FlowPoint> = flow
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let tr = flow.transitions.get(i);
                        FlowPoint {
                            title: p.track.tags.title.clone().into(),
                            bpm: p.bpm.map(|b| format!("{b:.0}")).unwrap_or_default().into(),
                            key: p.key.map(|k| k.format(st.config.key_notation)).unwrap_or_default().into(),
                            energy: p.energy.unwrap_or(0.3),
                            start: fmt_time(p.start_offset).into(),
                            tempo_fit: tr.map(|t| fit(t.tempo_fit)).unwrap_or("none").into(),
                            key_fit: tr.map(|t| fit(t.key_fit)).unwrap_or("none").into(),
                            change: tr.and_then(|t| t.bpm_change_pct).map(|p| format!("{p:+.1}%")).unwrap_or_default().into(),
                        }
                    })
                    .collect();
                // Energy relative to the set, so differences are visible.
                let mut pts = pts;
                let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.energy), b.max(p.energy)));
                if hi > lo {
                    for p in &mut pts {
                        p.energy = 0.15 + 0.85 * (p.energy - lo) / (hi - lo);
                    }
                }
                self.ui.set_flow(model(pts));
                self.ui.set_flow_total(fmt_time(flow.total_duration).into());
            }
            self.ui.set_flow_selected(st.flow_selected.map(|i| i as i32).unwrap_or(-1));
            let note = st.flow_selected.and_then(|i| entries.get(i)).map(|e| e.note.clone()).unwrap_or_default();
            self.ui.set_transition_note(note.into());
        }
    }

    pub(crate) fn refresh_detail(&self) {
        let st = self.state.borrow();
        let Some(id) = st.current else {
            self.ui.set_has_track(false);
            return;
        };
        let Ok(t) = st.src().track(id) else {
            self.ui.set_has_track(false);
            return;
        };
        let analysis = st.src().analysis(id).unwrap_or_default();
        let grid = st.src().beatgrid_with_source(id).ok().flatten();
        let cues = st.src().cues(id).unwrap_or_default();
        let duration = t.duration.max(0.001);
        self.ui.set_has_track(true);
        self.ui.set_track_title(t.tags.title.clone().into());
        self.ui.set_track_artist(t.tags.artist.clone().into());
        self.ui.set_track_bpm(t.bpm.map(|b| format!("{b:.2}")).unwrap_or_default().into());
        self.ui.set_track_key(st.key_text(&t).into());
        self.ui.set_track_duration(fmt_time(t.duration).into());
        self.ui.set_track_analysed(t.analysis_version >= od_analysis::ANALYSIS_VERSION);
        self.ui.set_grid_source(
            od_i18n::tr(&grid.as_ref().map(|(_, s)| s.as_str().to_uppercase()).unwrap_or_else(|| "NONE".into())).to_string().into(),
        );
        self.ui.set_grid_locked(grid.as_ref().is_some_and(|(g, _)| g.locked));
        let scale = self.ui.window().scale_factor();
        self.ui.set_track_overview(render::overview(&analysis.waveform, &analysis.beatgrid, t.duration, (300.0 * scale) as u32, (92.0 * scale) as u32));
        let marks: Vec<CueMark> = cues
            .iter()
            .map(|c| {
                let slot = match c.kind {
                    CueKind::Hot(s) => s as i32,
                    CueKind::Memory => -1,
                };
                CueMark {
                    slot,
                    letter: c.kind.hot_slot_letter().map(|l| l.to_string()).unwrap_or_default().into(),
                    pos: (c.start / duration) as f32,
                    end: (c.loop_end.unwrap_or(c.start) / duration) as f32,
                    is_loop: c.is_loop(),
                    time: fmt_cue_time(c.start).into(),
                    label: c.label.clone().into(),
                }
            })
            .collect();
        self.ui.set_track_cues(model(marks));
        let slots: Vec<HotCueSlot> = (0..8u8)
            .map(|s| {
                let c = cues.iter().find(|c| c.kind == CueKind::Hot(s));
                HotCueSlot {
                    slot: s as i32,
                    letter: ((b'A' + s) as char).to_string().into(),
                    set: c.is_some(),
                    time: c.map(|c| fmt_cue_time(c.start)).unwrap_or_default().into(),
                    label: c.map(|c| c.label.clone()).unwrap_or_default().into(),
                }
            })
            .collect();
        self.ui.set_hot_cues_top(model(slots[..4].to_vec()));
        self.ui.set_hot_cues_bottom(model(slots[4..].to_vec()));
        self.ui.set_cursor((st.cursor / duration) as f32);
    }

    fn refresh_devices(&self) {
        let st = self.state.borrow();
        let rows: Vec<DeviceRow> = st
            .statuses
            .iter()
            .map(|s| {
                let (state, detail, progress) = match &s.state {
                    DeviceState::Disconnected => ("disconnected", "Not connected".to_string(), 0.0),
                    DeviceState::InSync => (
                        "in-sync",
                        match s.device.last_sync.as_deref() {
                            Some(t) => format!("In sync · {}", t.get(11..16).unwrap_or(t)),
                            None => "In sync".to_string(),
                        },
                        1.0,
                    ),
                    DeviceState::OutOfDate { pending } => ("pending", format!("{pending} change{} waiting", if *pending == 1 { "" } else { "s" }), 0.0),
                    DeviceState::Syncing(p) => match p {
                        od_devices::Progress::Copying { done_bytes, total_bytes, current } => (
                            "syncing",
                            format!("Copying {current}"),
                            if *total_bytes > 0 { *done_bytes as f32 / *total_bytes as f32 } else { 0.0 },
                        ),
                        od_devices::Progress::Writing { format } => ("syncing", format!("Writing {format} data"), 0.95),
                        _ => ("syncing", "Preparing".to_string(), 0.02),
                    },
                    DeviceState::Error(e) => ("error", e.clone(), 0.0),
                };
                let mut formats = Vec::new();
                if s.device.formats.pioneer {
                    formats.push("PIONEER");
                }
                if s.device.formats.opendecks {
                    formats.push("OPENDECKS");
                }
                DeviceRow {
                    id: s.device.id.0 as i32,
                    name: s.device.name.clone().into(),
                    state: state.into(),
                    detail: od_i18n::translate_message(&detail).into(),
                    progress,
                    space: s.volume.as_ref().map(|v| od_i18n::trf("{} FREE OF {}", &[&fmt_bytes(v.free_bytes), &fmt_bytes(v.total_bytes)])).unwrap_or_default().into(),
                    formats: formats.join(" + ").into(),
                    connected: s.volume.is_some(),
                }
            })
            .collect();
        // OPENDECKS_DEMO_SYNC=0.4 shows the first connected stick mid-sync
        // (documentation screenshots and design review).
        let mut rows = rows;
        if let Some(p) = std::env::var("OPENDECKS_DEMO_SYNC").ok().and_then(|v| v.parse::<f32>().ok()) {
            if let Some(r) = rows.iter_mut().find(|r| r.connected) {
                r.state = "syncing".into();
                r.progress = p.clamp(0.0, 1.0);
                r.detail = od_i18n::translate_message("Copying {}").replace("{}", "…").into();
            }
        }
        self.ui.set_devices(model(rows));
        let vols: Vec<VolumeRow> = st
            .volumes
            .iter()
            .enumerate()
            .map(|(i, v)| VolumeRow {
                index: i as i32,
                name: v.name.clone().into(),
                space: od_i18n::trf("{} FREE OF {}", &[&fmt_bytes(v.free_bytes), &fmt_bytes(v.total_bytes)]).into(),
            })
            .collect();
        self.ui.set_volumes(model(vols));

        // Device detail.
        let Some(id) = st.device_selected else {
            self.ui.set_device_selected(-1);
            return;
        };
        let Ok(device) = st.lib.device(id) else {
            self.ui.set_device_selected(-1);
            return;
        };
        self.ui.set_device_selected(id.0 as i32);
        self.ui.set_device_name(device.name.clone().into());
        self.ui.set_device_pioneer(device.formats.pioneer);
        self.ui.set_device_opendecks(device.formats.opendecks);
        let assigned: HashSet<NodeId> = st.lib.device_nodes(id).unwrap_or_default().into_iter().collect();
        let mut choices = Vec::new();
        fn walk(lib: &Library, parent: Option<NodeId>, depth: i32, assigned: &HashSet<NodeId>, out: &mut Vec<NodeChoice>) {
            for n in lib.children(parent).unwrap_or_default() {
                out.push(NodeChoice { id: n.id.0 as i32, name: n.name.clone().into(), depth, checked: assigned.contains(&n.id) });
                if n.kind == NodeKind::Folder {
                    walk(lib, Some(n.id), depth + 1, assigned, out);
                }
            }
        }
        walk(&st.lib, None, 0, &assigned, &mut choices);
        self.ui.set_device_content(model(choices));
        let props: Vec<ProposalRow> = st
            .proposals
            .iter()
            .enumerate()
            .filter(|(_, (d, _))| *d == id)
            .map(|(i, (_, p))| match p {
                Proposal::Cues { track, stick, library } => {
                    let title = st.lib.track(*track).map(|t| t.tags.title).unwrap_or_else(|_| format!("Track {track}"));
                    ProposalRow {
                        index: i as i32,
                        title: title.into(),
                        detail: format!("Player set {} cue(s); library has {}", stick.len(), library.len()).into(),
                        kind: "cues".into(),
                    }
                }
                Proposal::History { name, tracks } => ProposalRow {
                    index: i as i32,
                    title: format!("History: {name}").into(),
                    detail: format!("{} tracks played", tracks.len()).into(),
                    kind: "history".into(),
                },
            })
            .collect();
        self.ui.set_proposals(model(props));
    }

    /// Recomputes the browsable sticks from connected volumes.
    fn update_sticks(&self) {
        let changed = {
            let mut st = self.state.borrow_mut();
            let mut sticks: Vec<(String, PathBuf)> = st
                .statuses
                .iter()
                .filter_map(|s| s.volume.as_ref().map(|v| (s.device.name.clone(), v.mount_point.clone())))
                .chain(st.volumes.iter().map(|v| {
                    let name = od_format_native::read_marker(&v.mount_point).ok().flatten().map(|m| m.name).unwrap_or_else(|| v.name.clone());
                    (name, v.mount_point.clone())
                }))
                .filter(|(_, root)| od_format_native::library_path(root).exists())
                .collect();
            sticks.sort();
            sticks.dedup_by(|a, b| a.1 == b.1);
            if sticks == st.sticks {
                false
            } else {
                // Leave a stick view whose stick went away.
                let active_root = st.stick.as_ref().map(|(i, _)| st.sticks.get(*i).map(|s| s.1.clone()));
                st.sticks = sticks;
                if let Some(root) = active_root.flatten() {
                    match st.sticks.iter().position(|s| s.1 == root) {
                        Some(i) => {
                            if let Some(s) = st.stick.as_mut() {
                                s.0 = i;
                            }
                        }
                        None => {
                            st.stick = None;
                            st.node = None;
                            st.current = None;
                            st.selection.clear();
                        }
                    }
                }
                true
            }
        };
        if changed {
            // Standalone: a stick becomes the source as soon as it is plugged in.
            let auto = {
                let st = self.state.borrow();
                st.stick.is_none() && !st.sticks.is_empty() && (st.config.kiosk || st.lib.track_count().unwrap_or(0) == 0)
            };
            if auto {
                self.ui.invoke_select_node(STICK_ROOT);
            }
            self.state.borrow_mut().load_rows();
            self.refresh_tree();
            self.refresh_tracks();
            self.refresh_detail();
        }
    }

    /// Selects the next (or previous) list in the tree; used by the
    /// controller's back button in kiosk mode.
    pub(crate) fn cycle_list(&self, forward: bool) {
        let ids: Vec<i32> = self.ui.get_tree().iter().map(|r| r.id).collect();
        if ids.is_empty() {
            return;
        }
        let cur = self.ui.get_selected_node();
        let i = ids.iter().position(|&x| x == cur).unwrap_or(0);
        let next = if forward { (i + 1) % ids.len() } else { (i + ids.len() - 1) % ids.len() };
        self.ui.invoke_select_node(ids[next]);
    }

    fn refresh_recordings(&self) {
        let st = self.state.borrow();
        let mut rows = Vec::new();
        if let Ok(mut stmt) = st.lib.conn().prepare("SELECT id, path, started, duration FROM recording ORDER BY started DESC") {
            if let Ok(iter) = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, f64>(3)?))
            }) {
                for (id, path, started, duration) in iter.flatten() {
                    let name = PathBuf::from(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(path.clone());
                    rows.push(RecordingRow {
                        id: id as i32,
                        name: name.into(),
                        detail: format!("{}  ·  {}", started.replace('T', " ").trim_end_matches('Z'), fmt_time(duration)).into(),
                        path: path.into(),
                    });
                }
            }
        }
        self.ui.set_recordings(model(rows));
    }

    fn refresh_status(&self) {
        let st = self.state.borrow();
        let journal = st.lib.journal(1).unwrap_or_default();
        self.ui.set_undo_label(
            journal.first().filter(|j| !j.undone).map(|j| od_i18n::translate_message(&j.label).to_uppercase()).unwrap_or_default().into(),
        );
        let (done, total) = st.analysis;
        if total > 0 && done < total {
            self.ui.set_analysis_progress(done as f32 / total as f32);
            self.ui.set_analysis_text(od_i18n::trf("ANALYSING {}/{}", &[&done, &total]).into());
        } else {
            self.ui.set_analysis_progress(0.0);
            self.ui.set_analysis_text(SharedString::new());
        }
    }

    fn status(&self, text: impl Into<String>) {
        self.ui.set_status_text(od_i18n::translate_message(&text.into()).into());
    }

    // ── Periodic work ───────────────────────────────────────────────────

    fn tick(&self) {
        // Worker progress.
        let mut need_refresh = false;
        for e in self.worker_events.try_iter() {
            match e {
                WorkerEvent::ImportProgress { done, total } => self.status(format!("Adding music: {done} of {total} files")),
                WorkerEvent::ImportFinished { added, failed } => {
                    self.status(if failed > 0 { format!("Added {added} tracks, {failed} files could not be read") } else { format!("Added {added} tracks") });
                    need_refresh = true;
                }
                WorkerEvent::AnalysisProgress { done, total } => {
                    self.state.borrow_mut().analysis = (done, total);
                    need_refresh = true;
                }
                WorkerEvent::AnalysisIdle => {
                    let was = self.state.borrow().analysis;
                    if was.1 > 0 {
                        self.state.borrow_mut().analysis = (0, 0);
                        self.status("Analysis complete");
                        need_refresh = true;
                    }
                }
                WorkerEvent::Message(m) => self.status(m),
            }
        }
        // Device events.
        let mut devices_changed = false;
        for e in self.devices.events().try_iter() {
            let mut st = self.state.borrow_mut();
            match e {
                Event::Status(s) => {
                    st.statuses = s;
                    devices_changed = true;
                }
                Event::UnregisteredVolumes(v) => {
                    st.volumes = v;
                    devices_changed = true;
                }
                Event::SyncFinished { device, report } => {
                    let name = st.lib.device(device).map(|d| d.name).unwrap_or_default();
                    drop(st);
                    self.status(format!(
                        "{name} synced: {} copied, {} updated, {} removed{}",
                        report.copied,
                        report.updated,
                        report.removed,
                        if report.skipped_missing.is_empty() { String::new() } else { format!(", {} missing files skipped", report.skipped_missing.len()) }
                    ));
                }
                Event::SyncFailed { error, .. } => {
                    drop(st);
                    self.status(format!("Sync failed: {error}"));
                }
                Event::Proposals { device, proposals } => {
                    st.proposals.retain(|(d, p)| *d != device || proposals.iter().all(|q| !same_target(p, q)));
                    st.proposals.extend(proposals.into_iter().map(|p| (device, p)));
                    devices_changed = true;
                    drop(st);
                    self.status("A player changed cues or wrote history on a stick — review it under Devices");
                }
                Event::Ejected { device } => {
                    let name = st.lib.device(device).map(|d| d.name).unwrap_or_default();
                    drop(st);
                    self.status(format!("{name} can be removed safely"));
                }
                Event::EjectFailed { error, .. } => {
                    drop(st);
                    self.status(format!("Eject failed: {error}"));
                }
                Event::Registered { .. } => need_refresh = true,
            }
        }
        if devices_changed {
            self.update_sticks();
            self.refresh_devices();
        }
        // Changes made through other connections.
        let seq = self.state.borrow().lib.change_seq().unwrap_or(0);
        let changed = {
            let mut st = self.state.borrow_mut();
            let c = seq != st.last_seq;
            st.last_seq = seq;
            c
        };
        if changed || need_refresh {
            self.state.borrow_mut().load_rows();
            self.refresh_tree();
            self.refresh_tracks();
            self.refresh_detail();
            self.refresh_status();
        }
    }

    // ── Actions ─────────────────────────────────────────────────────────

    fn after_edit(&self, label: &str) {
        self.state.borrow_mut().load_rows();
        self.refresh_tree();
        self.refresh_tracks();
        self.refresh_detail();
        self.refresh_status();
        if !label.is_empty() {
            self.status(label.to_string());
        }
    }

    fn report<T>(&self, r: od_library::Result<T>) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) => {
                self.status(format!("{e}"));
                None
            }
        }
    }

    fn select_row(&self, i: usize, toggle: bool, range: bool) {
        {
            let mut st = self.state.borrow_mut();
            if range {
                let a = st.anchor.unwrap_or(i);
                st.selection = (a.min(i)..=a.max(i)).collect();
            } else if toggle {
                if !st.selection.remove(&i) {
                    st.selection.insert(i);
                }
                st.anchor = Some(i);
            } else {
                st.selection = [i].into_iter().collect();
                st.anchor = Some(i);
            }
            let new = st.rows.get(i).map(|t| t.id);
            if new != st.current {
                st.current = new;
                st.cursor = 0.0;
            }
            if st.node_kind() == Some(NodeKind::Set) {
                st.flow_selected = Some(i);
            }
        }
        self.refresh_tracks();
        self.refresh_detail();
    }

    /// Tells the user that stick tracks cannot be edited; returns true when
    /// the current view is read-only.
    pub(crate) fn guard_read_only(&self) -> bool {
        if self.state.borrow().read_only() {
            self.status("Tracks on a stick are read-only here — edit them in your library and the stick follows");
            true
        } else {
            false
        }
    }

    fn edit_grid(&self, f: impl FnOnce(&mut Beatgrid, f64)) {
        if self.guard_read_only() {
            return;
        }
        let (id, cursor) = {
            let st = self.state.borrow();
            match st.current {
                Some(id) => (id, st.cursor),
                None => return,
            }
        };
        let grid = self.state.borrow().lib.beatgrid(id).ok().flatten();
        let Some(mut grid) = grid else {
            self.status("This track has no beatgrid yet — analyse it first");
            return;
        };
        f(&mut grid, cursor);
        let r = self.state.borrow_mut().lib.set_beatgrid(id, grid);
        if self.report(r).is_some() {
            self.after_edit("");
        }
    }

    fn wire(self: &Rc<Self>, worker_tx: crossbeam_channel::Sender<WorkerEvent>) {
        let ui = &self.ui;
        macro_rules! with_app {
            ($app:ident, $body:expr) => {{
                let weak = Rc::downgrade(self);
                move || {
                    if let Some($app) = weak.upgrade() {
                        $body
                    }
                }
            }};
        }
        macro_rules! with_app1 {
            ($app:ident, $a:ident : $t:ty, $body:expr) => {{
                let weak = Rc::downgrade(self);
                move |$a: $t| {
                    if let Some($app) = weak.upgrade() {
                        $body
                    }
                }
            }};
        }

        ui.on_select_section(with_app1!(app, s: SharedString, {
            app.ui.set_section(s.clone());
            if s == "devices" {
                let first = app.state.borrow().lib.devices().ok().and_then(|d| d.first().map(|d| d.id));
                let mut st = app.state.borrow_mut();
                if st.device_selected.is_none() {
                    st.device_selected = first;
                }
                drop(st);
                app.refresh_devices();
            }
            if s == "recordings" {
                app.refresh_recordings();
            }
        }));
        ui.on_set_mode(with_app1!(app, m: SharedString, { app.ui.set_mode(m); }));
        ui.on_select_node(with_app1!(app, id: i32, {
            {
                let mut st = app.state.borrow_mut();
                match decode_stick_id(id) {
                    Some((i, node)) => {
                        let reuse = st.stick.as_ref().is_some_and(|(j, _)| *j == i);
                        if !reuse {
                            let root = st.sticks.get(i).map(|s| s.1.clone());
                            st.stick = root.and_then(|r| od_format_native::Stick::open(&r).ok().flatten()).map(|s| (i, s));
                        }
                        st.node = node;
                        st.current = None;
                    }
                    None => {
                        if st.stick.take().is_some() {
                            st.current = None;
                        }
                        st.node = if id < 0 { None } else { Some(NodeId(id as i64)) };
                    }
                }
                st.selection.clear();
                st.flow_selected = None;
                st.search.clear();
                let in_list = st.node.is_some_and(|n| st.src().node(n).is_ok_and(|n| n.kind.has_entries()));
                st.sort = if in_list { "position".into() } else { "date_added".into() };
                st.descending = !in_list;
                if let (Some(n), false) = (st.node, st.read_only()) {
                    if st.lib.node(n).is_ok_and(|n| n.kind == NodeKind::Folder) {
                        st.expanded.insert(n);
                    }
                }
            }
            app.ui.set_search_text(SharedString::new());
            app.ui.set_section("library".into());
            app.state.borrow_mut().load_rows();
            app.refresh_tree();
            app.refresh_tracks();
        }));
        ui.on_toggle_node(with_app1!(app, id: i32, {
            {
                let mut st = app.state.borrow_mut();
                let n = NodeId(id as i64);
                if !st.expanded.remove(&n) {
                    st.expanded.insert(n);
                }
            }
            app.refresh_tree();
        }));
        ui.on_add_to_node(with_app1!(app, id: i32, {
            if app.guard_read_only() {
                return;
            }
            let ids = app.state.borrow().selected_ids();
            let name = app.state.borrow().lib.node(NodeId(id as i64)).map(|n| n.name).unwrap_or_default();
            let r = app.state.borrow_mut().lib.add_to_node(NodeId(id as i64), &ids, None);
            if app.report(r).is_some() {
                app.after_edit(&format!("Added {} track(s) to {name}", ids.len()));
            }
        }));
        ui.on_create_node(with_app1!(app, kind: SharedString, {
            if app.state.borrow().read_only() {
                let mut st = app.state.borrow_mut();
                st.stick = None;
                st.node = None;
                st.selection.clear();
            }
            let (kind, name) = match kind.as_str() {
                "folder" => (NodeKind::Folder, "New folder"),
                "set" => (NodeKind::Set, "New set"),
                "smart" => (NodeKind::Smart, "New smart list"),
                _ => (NodeKind::Playlist, "New playlist"),
            };
            let parent = {
                let st = app.state.borrow();
                st.node.and_then(|n| st.lib.node(n).ok()).filter(|n| n.kind == NodeKind::Folder).map(|n| n.id)
            };
            let selected = app.state.borrow().selected_ids();
            let r = app.state.borrow_mut().lib.create_node(parent, kind, name);
            if let Some(id) = app.report(r) {
                if kind.has_entries() && !selected.is_empty() && app.state.borrow().node.is_none() {
                    let _ = app.state.borrow_mut().lib.add_to_node(id, &selected, None);
                }
                {
                    let mut st = app.state.borrow_mut();
                    if let Some(p) = parent {
                        st.expanded.insert(p);
                    }
                    st.node = Some(id);
                    st.selection.clear();
                    st.sort = if kind.has_entries() { "position".into() } else { "date_added".into() };
                }
                app.after_edit("Created — click the title to rename it");
            }
        }));
        ui.on_rename_node(with_app1!(app, name: SharedString, {
            if app.guard_read_only() {
                return;
            }
            let node = app.state.borrow().node;
            if let Some(n) = node {
                let r = app.state.borrow_mut().lib.rename_node(n, name.trim());
                if app.report(r).is_some() {
                    app.after_edit("Renamed");
                }
            }
        }));
        ui.on_delete_node(with_app!(app, {
            if app.guard_read_only() {
                return;
            }
            let node = app.state.borrow().node;
            if let Some(n) = node {
                let r = app.state.borrow_mut().lib.delete_node(n);
                if app.report(r).is_some() {
                    app.state.borrow_mut().node = None;
                    app.after_edit("Deleted — ⌘Z brings it back");
                }
            }
        }));
        ui.on_remove_selected(with_app!(app, { app.remove_selected(); }));
        ui.on_search(with_app1!(app, q: SharedString, {
            app.state.borrow_mut().search = q.to_string();
            app.state.borrow_mut().load_rows();
            app.refresh_tracks();
        }));
        ui.on_set_smart_rule(with_app1!(app, text: SharedString, {
            if app.guard_read_only() {
                return;
            }
            match smart_text::parse(&text) {
                Ok(rule) => {
                    app.ui.set_smart_rule_error(SharedString::new());
                    let node = app.state.borrow().node;
                    if let Some(n) = node {
                        let r = app.state.borrow_mut().lib.set_smart_rule(n, rule);
                        if app.report(r).is_some() {
                            app.after_edit("Rules updated");
                        }
                    }
                }
                Err(e) => app.ui.set_smart_rule_error(e.into()),
            }
        }));
        ui.on_sort(with_app1!(app, col: SharedString, {
            {
                let mut st = app.state.borrow_mut();
                if st.sort == col.as_str() {
                    st.descending = !st.descending;
                } else {
                    st.sort = col.to_string();
                    st.descending = matches!(col.as_str(), "date_added" | "rating" | "bpm");
                }
                if st.sort == "position" {
                    st.descending = false;
                }
            }
            app.state.borrow_mut().load_rows();
            app.refresh_tracks();
        }));
        {
            let weak = Rc::downgrade(self);
            ui.on_row_clicked(move |i, toggle, range| {
                if let Some(app) = weak.upgrade() {
                    app.select_row(i as usize, toggle, range);
                }
            });
        }
        ui.on_row_activated(with_app1!(app, i: i32, {
            app.select_row(i as usize, false, false);
            crate::perform::load_selected_into_free_deck(&app);
        }));
        {
            let weak = Rc::downgrade(self);
            let worker_tx = worker_tx.clone();
            ui.on_import_folder(move || {
                let Some(app) = weak.upgrade() else { return };
                let Some(dirs) = rfd::FileDialog::new().set_title("Add music folder").pick_folders() else { return };
                let path = app.state.borrow().config.library_path.clone();
                spawn_import(path, dirs, worker_tx.clone(), app.analysis.wake.clone());
                app.status("Adding music…");
            });
        }
        ui.on_import_rekordbox(with_app!(app, { crate::import::run_rekordbox_import(&app); }));
        ui.on_seek(with_app1!(app, p: f32, {
            {
                let mut st = app.state.borrow_mut();
                let d = st.current.and_then(|id| st.src().track(id).ok()).map(|t| t.duration).unwrap_or(0.0);
                st.cursor = p as f64 * d;
                // Snap the cursor to the nearest beat when a grid exists.
                if let Some(g) = st.current.and_then(|id| st.src().beatgrid(id).ok().flatten()) {
                    if let Some(b) = g.beat_position(st.cursor) {
                        if let Some(t) = g.time_of_beat(b.round()) {
                            if (t - st.cursor).abs() < 0.08 {
                                st.cursor = t;
                            }
                        }
                    }
                }
            }
            crate::perform::preview_seek(&app);
            app.refresh_detail();
        }));
        ui.on_hot_cue(with_app1!(app, slot: i32, {
            let (id, cursor) = {
                let st = app.state.borrow();
                match st.current {
                    Some(id) => (id, st.cursor),
                    None => return,
                }
            };
            let cues = app.state.borrow().src().cues(id).unwrap_or_default();
            if let Some(c) = cues.iter().find(|c| c.kind == CueKind::Hot(slot as u8)) {
                app.state.borrow_mut().cursor = c.start;
                crate::perform::preview_seek(&app);
                app.refresh_detail();
            } else if app.guard_read_only() {
            } else {
                let mut cues = cues;
                cues.push(Cue::hot(slot as u8, cursor));
                let r = app.state.borrow_mut().lib.set_cues(id, cues);
                if app.report(r).is_some() {
                    app.after_edit(&format!("Hot cue {} set", (b'A' + slot as u8) as char));
                }
            }
        }));
        ui.on_remove_hot_cue(with_app1!(app, slot: i32, {
            if app.guard_read_only() {
                return;
            }
            let id = app.state.borrow().current;
            if let Some(id) = id {
                let mut cues = app.state.borrow().lib.cues(id).unwrap_or_default();
                cues.retain(|c| c.kind != CueKind::Hot(slot as u8));
                let r = app.state.borrow_mut().lib.set_cues(id, cues);
                if app.report(r).is_some() {
                    app.after_edit("Hot cue removed");
                }
            }
        }));
        ui.on_add_memory_cue(with_app!(app, {
            if app.guard_read_only() {
                return;
            }
            let (id, cursor) = {
                let st = app.state.borrow();
                match st.current {
                    Some(id) => (id, st.cursor),
                    None => return,
                }
            };
            let mut cues = app.state.borrow().lib.cues(id).unwrap_or_default();
            cues.push(Cue::memory(cursor));
            let r = app.state.borrow_mut().lib.set_cues(id, cues);
            if app.report(r).is_some() {
                app.after_edit("Memory cue added");
            }
        }));
        ui.on_grid_shift(with_app1!(app, ms: i32, { app.edit_grid(|g, _| g.shift(ms as f64 / 1000.0)); }));
        ui.on_grid_scale(with_app1!(app, f: f32, {
            app.edit_grid(|g, _| {
                let Some(first) = g.beats.first().copied() else { return };
                let bpm = g.dominant_bpm().unwrap_or(first.bpm) * f as f64;
                let end = g.beats.last().map(|b| b.time).unwrap_or(0.0) + 1.0;
                *g = Beatgrid::constant(bpm, first.time, first.bar_position, end);
            });
        }));
        ui.on_grid_downbeat(with_app!(app, {
            app.edit_grid(|g, cursor| {
                if let Some(i) = g.beat_position(cursor).map(|b| b.round().max(0.0) as usize) {
                    g.set_downbeat(i.min(g.beats.len().saturating_sub(1)));
                }
            });
        }));
        ui.on_grid_lock(with_app!(app, { app.edit_grid(|g, _| g.locked = !g.locked); }));
        ui.on_analyse_selected(with_app!(app, {
            if app.guard_read_only() {
                return;
            }
            let id = app.state.borrow().current;
            if let Some(id) = id {
                let _ = app.state.borrow().lib.conn().execute("UPDATE track SET analysis_version = 0 WHERE id = ?1", [id.0]);
                let _ = app.analysis.wake.send(());
                app.status("Queued for analysis");
            }
        }));
        ui.on_toggle_preview(with_app!(app, { crate::perform::toggle_preview(&app); }));
        ui.on_flow_select(with_app1!(app, i: i32, { app.select_row(i as usize, false, false); }));
        ui.on_set_transition_note(with_app1!(app, text: SharedString, {
            if app.guard_read_only() {
                return;
            }
            let (node, pos) = {
                let st = app.state.borrow();
                (st.node, st.flow_selected)
            };
            if let (Some(n), Some(p)) = (node, pos) {
                let r = app.state.borrow_mut().lib.set_entry_note(n, p, text.trim());
                if app.report(r).is_some() {
                    app.after_edit("Transition note saved");
                }
            }
        }));
        ui.on_sync_device(with_app1!(app, id: i32, { app.devices.send(Command::SyncNow(DeviceId(id as i64))); }));
        ui.on_rebuild_device(with_app1!(app, id: i32, { app.devices.send(Command::Rebuild(DeviceId(id as i64))); app.status("Rebuilding stick…"); }));
        ui.on_eject_device(with_app1!(app, id: i32, { app.devices.send(Command::Eject(DeviceId(id as i64))); app.status("Ejecting…"); }));
        ui.on_open_device(with_app1!(app, id: i32, {
            app.state.borrow_mut().device_selected = Some(DeviceId(id as i64));
            app.ui.set_section("devices".into());
            app.refresh_devices();
        }));
        ui.on_forget_device(with_app1!(app, id: i32, {
            let r = app.state.borrow_mut().lib.forget_device(DeviceId(id as i64));
            if app.report(r).is_some() {
                app.state.borrow_mut().device_selected = None;
                app.devices.send(Command::Refresh);
                app.status("Stick forgotten. Its files were left untouched.");
            }
        }));
        ui.on_register_volume(with_app1!(app, i: i32, {
            let (volume, node, name) = {
                let st = app.state.borrow();
                let node = st.node;
                let name = node.and_then(|n| st.lib.node(n).ok()).map(|n| n.name);
                (st.volumes.get(i as usize).cloned(), node, name)
            };
            let (Some(v), Some(n)) = (volume, node) else {
                app.status("Select the playlist, set or folder that should go on the stick first");
                return;
            };
            app.devices.send(Command::Register {
                mount_point: v.mount_point.clone(),
                name: v.name.clone(),
                formats: DeviceFormats::default(),
                nodes: vec![n],
            });
            app.status(format!("Registering {} with {} — the first sync starts now", v.name, name.unwrap_or_default()));
        }));
        {
            let weak = Rc::downgrade(self);
            ui.on_set_device_formats(move |pioneer, opendecks| {
                let Some(app) = weak.upgrade() else { return };
                let id = app.state.borrow().device_selected;
                if let Some(id) = id {
                    let name = app.state.borrow().lib.device(id).map(|d| d.name).unwrap_or_default();
                    let r = app.state.borrow_mut().lib.update_device(id, &name, DeviceFormats { pioneer, opendecks });
                    if app.report(r).is_some() {
                        app.refresh_devices();
                    }
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            ui.on_set_device_node(move |node, on| {
                let Some(app) = weak.upgrade() else { return };
                let id = app.state.borrow().device_selected;
                if let Some(id) = id {
                    let mut nodes = app.state.borrow().lib.device_nodes(id).unwrap_or_default();
                    let n = NodeId(node as i64);
                    nodes.retain(|x| *x != n);
                    if on {
                        nodes.push(n);
                    }
                    let r = app.state.borrow_mut().lib.set_device_nodes(id, &nodes);
                    if app.report(r).is_some() {
                        app.refresh_devices();
                        app.refresh_tree();
                    }
                }
            });
        }
        ui.on_rename_device(with_app1!(app, name: SharedString, {
            let id = app.state.borrow().device_selected;
            if let Some(id) = id {
                let formats = app.state.borrow().lib.device(id).map(|d| d.formats).unwrap_or_default();
                let r = app.state.borrow_mut().lib.update_device(id, name.trim(), formats);
                if app.report(r).is_some() {
                    app.devices.send(Command::Refresh);
                }
            }
        }));
        ui.on_accept_proposal(with_app1!(app, i: i32, { app.resolve_proposal(i as usize, true); }));
        ui.on_reject_proposal(with_app1!(app, i: i32, { app.resolve_proposal(i as usize, false); }));
        ui.on_undo(with_app!(app, { app.undo(); }));
        ui.on_redo(with_app!(app, { app.redo(); }));
        ui.on_reveal_recording(with_app1!(app, id: i32, {
            let path: Option<String> = app.state.borrow().lib.conn().query_row("SELECT path FROM recording WHERE id = ?1", [id], |r| r.get(0)).ok();
            if let Some(p) = path {
                let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
                let target = if cfg!(target_os = "macos") { vec!["-R".to_string(), p] } else { vec![PathBuf::from(p).parent().map(|d| d.display().to_string()).unwrap_or_default()] };
                let _ = std::process::Command::new(opener).args(target).spawn();
            }
        }));
        {
            let weak = Rc::downgrade(self);
            ui.on_key_pressed(move |text, cmd, shift| {
                let Some(app) = weak.upgrade() else { return false };
                app.key(text.as_str(), cmd, shift)
            });
        }
        crate::perform::wire(self);
    }

    fn key(&self, text: &str, cmd: bool, shift: bool) -> bool {
        use slint::platform::Key;
        let is = |k: Key| text == SharedString::from(k).as_str();
        if cmd && (text == "z" || text == "Z") {
            if shift { self.redo() } else { self.undo() }
            return true;
        }
        if is(Key::Delete) || is(Key::Backspace) {
            self.remove_selected();
            return true;
        }
        if is(Key::UpArrow) || is(Key::DownArrow) {
            let (len, cur) = {
                let st = self.state.borrow();
                (st.rows.len(), st.selection.iter().next_back().copied())
            };
            if len == 0 {
                return true;
            }
            let next = match cur {
                None => 0,
                Some(c) if is(Key::UpArrow) => c.saturating_sub(1),
                Some(c) => (c + 1).min(len - 1),
            };
            self.select_row(next, false, false);
            self.ui.set_focus_row(next as i32);
            return true;
        }
        if text == " " {
            crate::perform::toggle_preview(self);
            return true;
        }
        if !cmd && ("1"..="8").contains(&text) && text.len() == 1 {
            let slot = text.parse::<i32>().unwrap_or(1) - 1;
            self.ui.invoke_hot_cue(slot);
            return true;
        }
        false
    }

    fn remove_selected(&self) {
        if self.state.borrow().read_only() {
            return;
        }
        let (node, positions) = {
            let st = self.state.borrow();
            (st.node, st.selection.iter().copied().collect::<Vec<_>>())
        };
        let Some(n) = node else { return };
        if positions.is_empty() || !self.state.borrow().lib.node(n).is_ok_and(|n| n.kind.has_entries()) {
            return;
        }
        // Rows may be re-sorted; map rows back to entry positions.
        let entries = self.state.borrow().lib.entries(n).unwrap_or_default();
        let ids: Vec<TrackId> = self.state.borrow().selected_ids();
        let mut entry_positions: Vec<usize> = Vec::new();
        for id in &ids {
            if let Some(i) = (0..entries.len()).find(|i| entries[*i].track == *id && !entry_positions.contains(i)) {
                entry_positions.push(i);
            }
        }
        let r = self.state.borrow_mut().lib.remove_from_node(n, &entry_positions);
        if self.report(r).is_some() {
            self.state.borrow_mut().selection.clear();
            self.after_edit(&format!("Removed {} track(s) — ⌘Z to undo", entry_positions.len()));
        }
    }

    fn undo(&self) {
        let r = self.state.borrow_mut().lib.undo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Undid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to undo".into()));
        }
    }

    fn redo(&self) {
        let r = self.state.borrow_mut().lib.redo();
        if let Some(label) = self.report(r) {
            self.after_edit(&label.map(|l| format!("Redid: {}", od_i18n::translate_message(&l))).unwrap_or_else(|| "Nothing to redo".into()));
        }
    }

    fn resolve_proposal(&self, index: usize, accept: bool) {
        let entry = {
            let mut st = self.state.borrow_mut();
            if index >= st.proposals.len() {
                return;
            }
            st.proposals.remove(index)
        };
        let (device, proposal) = entry;
        let root = self
            .state
            .borrow()
            .statuses
            .iter()
            .find(|s| s.device.id == device)
            .and_then(|s| s.volume.as_ref().map(|v| v.mount_point.clone()));
        match proposal {
            Proposal::Cues { track, stick, .. } => {
                if let Some(root) = &root {
                    let _ = od_devices::PioneerWriter::release(root, track);
                }
                if accept {
                    let r = self.state.borrow_mut().lib.set_cues(track, od_devices::pioneer::accepted_cues(&stick));
                    self.report(r);
                } else {
                    // Rewrite the library's cues to the stick.
                    let cues = self.state.borrow().lib.cues(track).unwrap_or_default();
                    let r = self.state.borrow_mut().lib.set_cues(track, cues);
                    self.report(r);
                }
            }
            Proposal::History { name, tracks } => {
                if accept {
                    let existing: Vec<TrackId> = tracks.into_iter().filter(|t| self.state.borrow().lib.track(*t).is_ok()).collect();
                    let folder = {
                        let st = self.state.borrow();
                        st.lib.children(None).unwrap_or_default().into_iter().find(|n| n.kind == NodeKind::Folder && n.name == "History").map(|n| n.id)
                    };
                    let folder = match folder {
                        Some(f) => Some(f),
                        None => {
                            let r = self.state.borrow_mut().lib.create_node(None, NodeKind::Folder, "History");
                            self.report(r)
                        }
                    };
                    if let Some(f) = folder {
                        let r = self.state.borrow_mut().lib.create_node(Some(f), NodeKind::Playlist, &name);
                        if let Some(pl) = self.report(r) {
                            let _ = self.state.borrow_mut().lib.add_to_node(pl, &existing, None);
                        }
                    }
                }
            }
        }
        self.after_edit(if accept { "Change taken over" } else { "Kept the library version" });
        self.refresh_devices();
        self.devices.send(Command::SyncNow(device));
    }
}

fn same_target(a: &Proposal, b: &Proposal) -> bool {
    match (a, b) {
        (Proposal::Cues { track: x, .. }, Proposal::Cues { track: y, .. }) => x == y,
        (Proposal::History { name: x, tracks: tx }, Proposal::History { name: y, tracks: ty }) => x == y && tx == ty,
        _ => false,
    }
}

/// Helper used by other modules to read the current model length.
pub(crate) fn model_len<T: Clone + 'static>(m: &ModelRc<T>) -> usize {
    m.row_count()
}
