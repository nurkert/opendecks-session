//! The headless harness: a temporary library with generated audio, the real
//! app window on the Slint testing backend, mock time and synthetic input.

use crate::app::App;
use crate::state::AppConfig;
use i_slint_backend_testing::ElementHandle;
use od_core::TrackId;
use od_library::{Library, NodeId, NodeKind};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static BACKEND: Cell<bool> = const { Cell::new(false) };
}

/// The testing backend for this thread (each test runs on its own thread).
pub(crate) fn init_backend() {
    BACKEND.with(|b| {
        if !b.get() {
            i_slint_backend_testing::init_no_event_loop();
            b.set(true);
        }
    });
}

/// Process-wide environment for all UI tests: an empty data directory and a
/// simulated volumes directory (`OPENDECKS_VOLUMES_DIR`, read by the device
/// service) so no test sees the machine's real sticks or settings.
pub(crate) fn test_env() -> &'static Path {
    static ENV: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ENV.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("od-ui-tests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("volumes")).unwrap();
        std::fs::create_dir_all(base.join("data")).unwrap();
        // SAFETY: set once, before any app (and its threads) starts; the other
        // tests of this binary do not read these variables.
        unsafe {
            std::env::set_var("OPENDECKS_VOLUMES_DIR", base.join("volumes"));
            std::env::set_var("OPENDECKS_DATA_DIR", base.join("data"));
            std::env::set_var("OPENDECKS_MAPPINGS", base.join("data/mappings"));
        }
        base
    })
}

/// Writes a stereo 16-bit WAV: a 120 BPM kick-like click over a quiet tone
/// (`pitch` Hz), `secs` long. Distinct pitches give distinct content hashes.
pub(crate) fn write_wav(path: &Path, secs: f32, pitch: f32) {
    let rate = 44_100u32;
    let spec = hound::WavSpec { channels: 2, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    let beat = rate as f32 * 0.5;
    for n in 0..(secs * rate as f32) as usize {
        let t = n as f32 / rate as f32;
        let in_beat = (n as f32 % beat) / rate as f32;
        let click = if in_beat < 0.03 { (1.0 - in_beat / 0.03) * (2.0 * std::f32::consts::PI * 60.0 * in_beat).sin() * 0.8 } else { 0.0 };
        let tone = (2.0 * std::f32::consts::PI * pitch * t).sin() * 0.15;
        let s = ((click + tone) * i16::MAX as f32) as i16;
        w.write_sample(s).unwrap();
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}

/// A temporary library: three analysable tracks in the collection, a
/// playlist that already holds one of them, and an Explorer root with a
/// folder holding a file that is not in the collection.
#[allow(dead_code)]
pub(crate) struct Fixture {
    _dir: tempfile::TempDir,
    pub root: PathBuf,
    pub library: PathBuf,
    /// Explorer root (stands for the user folder).
    pub explorer_root: PathBuf,
    /// The folder below the Explorer root holding `outside_file`.
    pub outside_folder: PathBuf,
    pub outside_file: PathBuf,
    /// (id, title) in import order.
    pub tracks: Vec<(TrackId, String)>,
    pub playlist: NodeId,
}

pub(crate) const TITLES: [&str; 3] = ["Alpha Tune", "Bravo Tune", "Charlie Tune"];

impl Fixture {
    pub fn new() -> Self {
        test_env();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let library = root.join("library/library.sqlite");
        let mut lib = Library::open(&library).unwrap();
        let mut tracks = Vec::new();
        for (i, title) in TITLES.iter().enumerate() {
            let path = root.join(format!("music/track{}.wav", i + 1));
            write_wav(&path, 8.0, 220.0 + 110.0 * i as f32);
            let id = crate::workers::import_file(&mut lib, &path).unwrap();
            let mut tags = lib.track(id).unwrap().tags;
            tags.title = (*title).into();
            tags.artist = "Test Artist".into();
            lib.set_tags(id, tags).unwrap();
            tracks.push((id, title.to_string()));
        }
        let playlist = lib.create_node(None, NodeKind::Playlist, "Warm-up").unwrap();
        lib.add_to_node(playlist, &[tracks[0].0], None).unwrap();
        let explorer_root = root.join("home");
        let outside_folder = explorer_root.join("Crate");
        let outside_file = outside_folder.join("Outside Loop.wav");
        write_wav(&outside_file, 6.0, 660.0);
        lib.set_meta("explorer.roots", &serde_json::to_string(&vec![explorer_root.clone()]).unwrap()).unwrap();
        // English UI (menu titles are matched in English), recordings in the
        // temporary folder, not in ~/Music.
        let mut settings = crate::settings::Settings::default();
        settings.view.language = "en".into();
        settings.recording.folder = root.join("recordings").display().to_string();
        lib.set_meta(crate::settings::META_SETTINGS, &settings.to_json()).unwrap();
        drop(lib);
        Self { _dir: dir, root, library, explorer_root, outside_folder, outside_file, tracks, playlist }
    }

    /// A second connection to the library (assertions).
    pub fn lib(&self) -> Library {
        Library::open(&self.library).unwrap()
    }
}

/// A geometry snapshot of one labelled element.
#[derive(Clone, Debug)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn of(e: &ElementHandle) -> Self {
        let p = e.absolute_position();
        let s = e.size();
        Self { x: p.x, y: p.y, w: s.width, h: s.height }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn centre(&self) -> LogicalPosition {
        LogicalPosition::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    /// Area shared with `o` (0 when they only touch).
    pub fn overlap(&self, o: &Rect) -> f32 {
        let w = self.right().min(o.right()) - self.x.max(o.x);
        let h = self.bottom().min(o.bottom()) - self.y.max(o.y);
        if w > 0.5 && h > 0.5 { w * h } else { 0.0 }
    }
    pub fn contains(&self, o: &Rect, tol: f32) -> bool {
        o.x >= self.x - tol && o.y >= self.y - tol && o.right() <= self.right() + tol && o.bottom() <= self.bottom() + tol
    }
}

/// The running app plus its fixture.
pub(crate) struct Ui {
    pub app: Rc<App>,
    pub fx: Fixture,
    /// Steps a flow took (one per user action, `docs` step rule).
    pub steps: Cell<u32>,
}

impl Ui {
    /// Opens the app on `fx` at `size` (logical px). `kiosk` runs the
    /// Raspberry Pi kiosk configuration.
    pub fn start(fx: Fixture, size: (u32, u32), kiosk: bool) -> Self {
        init_backend();
        let mut cfg = AppConfig::new(fx.library.clone());
        cfg.null_audio = true;
        cfg.headless = true;
        cfg.kiosk = kiosk;
        cfg.window = Some(size);
        let app = App::new(cfg).expect("app starts headless");
        app.ui.show().unwrap();
        let ui = Self { app, fx, steps: Cell::new(0) };
        ui.resize(size);
        ui
    }

    pub fn open(size: (u32, u32)) -> Self {
        Self::start(Fixture::new(), size, false)
    }

    pub fn resize(&self, (w, h): (u32, u32)) {
        self.app.ui.window().set_size(LogicalSize::new(w as f32, h as f32));
        self.pump(400);
    }

    pub fn size(&self) -> (f32, f32) {
        let s = self.app.ui.window().size();
        let f = self.app.ui.window().scale_factor();
        (s.width as f32 / f, s.height as f32 / f)
    }

    /// Advances mock time by `ms` in 16 ms frames (timers, animations,
    /// change handlers).
    pub fn pump(&self, ms: u64) {
        let mut left = ms;
        while left > 0 {
            let step = left.min(16);
            i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(step));
            left -= step;
        }
    }

    /// Runs frames (mock time plus a little real time for the worker and
    /// engine threads) until `cond` holds or `secs` of real time pass.
    pub fn wait_until(&self, secs: f32, mut cond: impl FnMut(&Self) -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs_f32(secs);
        loop {
            if cond(self) {
                return true;
            }
            if Instant::now() > end {
                return false;
            }
            self.pump(16);
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    // ----- lookup ------------------------------------------------------

    pub fn find(&self, label: &str) -> Option<ElementHandle> {
        ElementHandle::find_by_accessible_label(&self.app.ui, label).next()
    }

    pub fn get(&self, label: &str) -> ElementHandle {
        self.find(label).unwrap_or_else(|| panic!("no element labelled `{label}`"))
    }

    pub fn has(&self, label: &str) -> bool {
        self.find(label).is_some()
    }

    /// The shown value of a labelled read-out (`accessible-value`).
    pub fn value(&self, label: &str) -> String {
        self.get(label).accessible_value().map(|v| v.to_string()).unwrap_or_default()
    }

    /// Every element with a convention label (docs/parity.md) in the main
    /// window and its open popups: label → handles.
    pub fn labelled(&self) -> BTreeMap<String, Vec<ElementHandle>> {
        labelled_in(&self.app.ui)
    }

    /// Elements whose label starts with `prefix`, with their values.
    pub fn with_prefix(&self, prefix: &str) -> Vec<(String, String, ElementHandle)> {
        let mut out = Vec::new();
        for (l, es) in self.labelled() {
            if l.starts_with(prefix) {
                for e in es {
                    let v = e.accessible_value().map(|v| v.to_string()).unwrap_or_default();
                    out.push((l.clone(), v, e));
                }
            }
        }
        out
    }

    /// The first element under `prefix` whose value equals `value`.
    pub fn by_value(&self, prefix: &str, value: &str) -> Option<(String, ElementHandle)> {
        self.with_prefix(prefix).into_iter().find(|(_, v, _)| v == value).map(|(l, _, e)| (l, e))
    }

    /// An item of an open (context) menu by its title. Native `MenuItem`s
    /// carry no accessible label; their text is the label. Elements inside
    /// The sidebar row of a playlist with the same name is skipped.
    pub fn menu_item(&self, title: &str) -> Option<ElementHandle> {
        // Same text in the sidebar tree (a playlist name): not the item.
        let t = title.to_string();
        let in_tree: Vec<Rect> = self
            .find("browser.tree")
            .map(|tree| tree.query_descendants().match_predicate(move |e| e.accessible_label().is_some_and(|l| l == t)).find_all())
            .unwrap_or_default()
            .iter()
            .map(Rect::of)
            .collect();
        ElementHandle::find_by_accessible_label(&self.app.ui, title)
            .filter(|e| e.accessible_role() == Some(i_slint_backend_testing::AccessibleRole::Text))
            .find(|e| {
                let r = Rect::of(e);
                !in_tree.iter().any(|t| (t.x - r.x).abs() < 0.5 && (t.y - r.y).abs() < 0.5 && (t.w - r.w).abs() < 0.5)
            })
    }

    /// Clicks an item of an open menu (popups have their own coordinates,
    /// so the testing API's own click maps the position).
    pub fn click_menu_item(&self, title: &str) -> bool {
        // Submenus open asynchronously; give them a moment under load.
        let mut found = self.menu_item(title);
        for _ in 0..20 {
            if found.is_some() {
                break;
            }
            self.pump(50);
            found = self.menu_item(title);
        }
        let Some(e) = found else { return false };
        self.step();
        e.mock_single_click(PointerEventButton::Left);
        self.pump(150);
        true
    }

    // ----- input (each call is one step of the step rule) ------------------

    fn step(&self) {
        self.steps.set(self.steps.get() + 1);
    }

    fn window(&self) -> &slint::Window {
        self.app.ui.window()
    }

    pub fn press_at(&self, p: LogicalPosition, button: PointerEventButton) {
        self.window().dispatch_event(WindowEvent::PointerMoved { position: p });
        self.window().dispatch_event(WindowEvent::PointerPressed { position: p, button });
    }

    pub fn release_at(&self, p: LogicalPosition, button: PointerEventButton) {
        self.window().dispatch_event(WindowEvent::PointerReleased { position: p, button });
    }

    pub fn click_at(&self, p: LogicalPosition, button: PointerEventButton) {
        self.press_at(p, button);
        self.pump(50);
        self.release_at(p, button);
        self.pump(50);
    }

    /// Click at the element's centre.
    pub fn click_el(&self, e: &ElementHandle) {
        self.step();
        self.click_at(Rect::of(e).centre(), PointerEventButton::Left);
    }

    pub fn click(&self, label: &str) {
        let e = self.get(label);
        self.click_el(&e);
    }

    pub fn right_click_el(&self, e: &ElementHandle) {
        self.step();
        self.click_at(Rect::of(e).centre(), PointerEventButton::Right);
        self.pump(100);
    }

    pub fn double_click_el(&self, e: &ElementHandle) {
        self.step();
        let p = Rect::of(e).centre();
        for _ in 0..2 {
            self.press_at(p, PointerEventButton::Left);
            self.pump(16);
            self.release_at(p, PointerEventButton::Left);
            self.pump(16);
        }
        self.pump(100);
    }

    pub fn double_click(&self, label: &str) {
        let e = self.get(label);
        self.double_click_el(&e);
    }

    /// Drags from the centre of `from` to `to` in small moves (≥ 2 frames
    /// per move so pointer-following code sees intermediate positions).
    pub fn drag_points(&self, from: LogicalPosition, to: LogicalPosition) {
        self.step();
        self.press_at(from, PointerEventButton::Left);
        self.pump(16);
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let n = ((dx * dx + dy * dy).sqrt() / 4.0).ceil().max(4.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            self.window().dispatch_event(WindowEvent::PointerMoved { position: LogicalPosition::new(from.x + dx * t, from.y + dy * t) });
            self.pump(16);
        }
        self.release_at(to, PointerEventButton::Left);
        self.pump(100);
    }

    pub fn drag_el(&self, e: &ElementHandle, to: LogicalPosition) {
        self.drag_points(Rect::of(e).centre(), to);
    }

    /// Mouse wheel over the element (`dy` > 0 scrolls up / away).
    pub fn scroll(&self, label: &str, dy: f32) {
        self.step();
        let p = Rect::of(&self.get(label)).centre();
        self.window().dispatch_event(WindowEvent::PointerMoved { position: p });
        self.window().dispatch_event(WindowEvent::PointerScrolled { position: p, delta_x: 0.0, delta_y: dy });
        self.pump(50);
    }

    /// Scrolls the scrollable `container` (wheel, one step) until the element
    /// found by `find` lies fully inside it. Returns the element.
    pub fn scroll_into_view(&self, container: &str, find: impl Fn(&Self) -> Option<ElementHandle>) -> Option<ElementHandle> {
        let c = Rect::of(&self.get(container));
        let e = find(self)?;
        let r = Rect::of(&e);
        if c.contains(&r, 0.5) {
            return Some(e);
        }
        let dy = if r.bottom() > c.bottom() { c.bottom() - r.bottom() - 4.0 } else { c.y - r.y + 4.0 };
        self.step();
        let p = c.centre();
        self.window().dispatch_event(WindowEvent::PointerMoved { position: p });
        self.window().dispatch_event(WindowEvent::PointerScrolled { position: p, delta_x: 0.0, delta_y: dy });
        self.pump(300);
        find(self)
    }

    /// Types `text` into the focused element (one step).
    pub fn type_text(&self, text: &str) {
        self.step();
        for c in text.chars() {
            let s: slint::SharedString = c.to_string().into();
            self.window().dispatch_event(WindowEvent::KeyPressed { text: s.clone() });
            self.window().dispatch_event(WindowEvent::KeyReleased { text: s });
        }
        self.pump(50);
    }

    /// Presses one key (`slint::platform::Key` as text) (one step).
    pub fn key(&self, key: slint::platform::Key) {
        self.step();
        let s: slint::SharedString = key.into();
        self.window().dispatch_event(WindowEvent::KeyPressed { text: s.clone() });
        self.window().dispatch_event(WindowEvent::KeyReleased { text: s });
        self.pump(50);
    }
}

/// Labelled elements of any component (main window, Preferences window).
pub(crate) fn labelled_in(root: &impl i_slint_backend_testing::ElementRoot) -> BTreeMap<String, Vec<ElementHandle>> {
    let mut out: BTreeMap<String, Vec<ElementHandle>> = BTreeMap::new();
    let found = root
        .root_element()
        .query_descendants()
        .match_predicate(|e| e.accessible_label().is_some_and(|l| od_parity::labels::is_valid_label(&l)))
        .find_all();
    for e in found {
        if let Some(l) = e.accessible_label() {
            out.entry(l.to_string()).or_default().push(e);
        }
    }
    out
}

/// Interactive accessible roles (touch-target rule applies).
pub(crate) fn interactive(e: &ElementHandle) -> bool {
    use i_slint_backend_testing::AccessibleRole as R;
    matches!(
        e.accessible_role(),
        Some(R::Button | R::Checkbox | R::Combobox | R::Slider | R::Spinbox | R::Tab | R::TextInput | R::Switch | R::ListItem | R::RadioButton)
    )
}
