//! The application: owns the window, the domain state, the per-module
//! runtime parts and the workers, and wires the feature modules together.
//!
//! Each feature module exposes `wire(app)` for its callbacks and refresh
//! functions that project [`AppState`] into its view model.

use crate::state::{AppConfig, AppState};
use slint::ComponentHandle;
use crate::workers::{AnalysisWorker, WorkerEvent};
use crate::{AppWindow, browser, controller, decks, devices, engine, explorer, fx, mixer, player, preferences, recorder, sampler, shell, update};
use crossbeam_channel::{Receiver, Sender, unbounded};
use od_devices::{DeviceManager, ManagerConfig};
use od_library::Library;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Builds a Slint callback that runs `body` with a strong reference to the
/// app while it is alive: `handler!(app, |a, x: i32| a.do_it(x))`.
macro_rules! handler {
    ($app:expr, |$a:ident $(, $arg:ident : $t:ty)*| $body:expr) => {{
        let weak = std::rc::Rc::downgrade($app);
        move |$($arg: $t),*| {
            if let Some($a) = weak.upgrade() {
                #[allow(clippy::unused_unit)]
                {
                    $body;
                }
            }
        }
    }};
}
pub(crate) use handler;

pub struct App {
    pub ui: AppWindow,
    /// Domain state (library, browser, player, devices, explorer).
    pub(crate) state: RefCell<AppState>,
    /// Audio engine and host.
    pub(crate) engine: RefCell<engine::EngineHost>,
    /// Loaded tracks, stems and the deck view models.
    pub(crate) decks: RefCell<decks::Decks>,
    pub(crate) mixer: mixer::MixerView,
    /// Beat FX panel rows and tap tempo.
    pub(crate) fx: fx::FxRuntime,
    /// Sampler slots, bank and panel rows.
    pub(crate) sampler: sampler::SamplerRuntime,
    pub(crate) recorder: RefCell<recorder::RecorderState>,
    pub(crate) controller: RefCell<controller::Controllers>,
    pub(crate) browser: browser::BrowserViews,
    pub(crate) explorer: explorer::ExplorerRuntime,
    pub(crate) device_manager: DeviceManager,
    pub(crate) analysis: AnalysisWorker,
    pub(crate) worker_tx: Sender<WorkerEvent>,
    /// The Preferences window, created when first opened.
    pub(crate) prefs: RefCell<preferences::Prefs>,
    /// Held keyboard actions (pitch bend): (action, deck).
    pub(crate) held: RefCell<Vec<(String, usize)>>,
    /// Update check and install (SPEC §14.1).
    pub(crate) update: RefCell<update::UpdateRuntime>,
    worker_events: Receiver<WorkerEvent>,
    tick_timer: slint::Timer,
}

impl App {
    pub fn new(config: AppConfig) -> anyhow::Result<Rc<Self>> {
        let lib = Library::open(&config.library_path)?;
        let state = AppState::new(lib, config);
        if state.settings.advanced.backup_on_start && (state.lib.backups().map(|b| b.is_empty()).unwrap_or(true) || state.lib.track_count()? > 0) {
            if let Err(e) = state.lib.backup_now("start") {
                tracing::warn!(error = %e, "start-up backup failed");
            }
        }
        // UI scale (Preferences > View); read by Slint when the window is created.
        let scale = state.settings.view.scale;
        if std::env::var_os("SLINT_SCALE_FACTOR").is_none() && (scale - 1.0).abs() > 0.01 {
            // SAFETY: no other thread has been started yet.
            unsafe { std::env::set_var("SLINT_SCALE_FACTOR", scale.to_string()) };
        }
        let ui = AppWindow::new()?;
        shell::setup_window(&ui, &state.config, &state.settings);
        let (wtx, wrx) = unbounded();
        let analysis = AnalysisWorker::start(state.config.library_path.clone(), preferences::analysis_control(&state.settings, state.config.lite), wtx.clone());
        let device_manager = DeviceManager::start(ManagerConfig::new(state.config.library_path.clone()))?;
        let app = Rc::new(App {
            ui,
            state: RefCell::new(state),
            prefs: RefCell::new(preferences::Prefs::default()),
            held: RefCell::new(Vec::new()),
            update: RefCell::new(update::UpdateRuntime::default()),
            engine: RefCell::new(engine::EngineHost::default()),
            decks: RefCell::new(decks::Decks::default()),
            mixer: mixer::MixerView::default(),
            fx: fx::FxRuntime::default(),
            sampler: sampler::SamplerRuntime::default(),
            recorder: RefCell::new(recorder::RecorderState::default()),
            controller: RefCell::new(controller::Controllers::default()),
            browser: browser::BrowserViews::default(),
            explorer: explorer::ExplorerRuntime::default(),
            device_manager,
            analysis,
            worker_tx: wtx,
            worker_events: wrx,
            tick_timer: slint::Timer::default(),
        });
        app.bind_models();
        shell::wire(&app);
        browser::wire(&app);
        explorer::wire(&app);
        player::wire(&app);
        devices::wire(&app);
        recorder::wire(&app);
        if !app.state.borrow().config.headless {
            controller::start(&app);
        }
        decks::wire(&app);
        mixer::wire(&app);
        fx::wire(&app);
        sampler::wire(&app);
        preferences::wire(&app);
        crate::appearance::wire(&app);
        update::wire(&app);
        engine::start(&app);
        decks::autoload(&app);

        let _ = app.state.borrow_mut().lib.check_missing();
        shell::restore_view(&app);
        app.refresh_all();
        let weak = Rc::downgrade(&app);
        app.tick_timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            if let Some(app) = weak.upgrade() {
                app.tick();
            }
        });
        Ok(app)
    }

    /// Binds the persistent models to their properties once.
    fn bind_models(&self) {
        self.ui.set_tracks(self.browser.tracks.handle());
        self.ui.set_channels(self.mixer.channels.handle());
        let d = self.decks.borrow();
        self.ui.set_decks(d.views.all.handle());
        self.ui.set_decks_left(d.views.left.handle());
        self.ui.set_decks_right(d.views.right.handle());
    }

    pub fn run(self: &Rc<Self>) -> anyhow::Result<()> {
        self.ui.run()?;
        shell::remember_view(self);
        Ok(())
    }

    pub(crate) fn refresh_all(&self) {
        self.state.borrow_mut().load_rows();
        browser::refresh_tree(self);
        browser::refresh_tracks(self);
        player::refresh_detail(self);
        devices::refresh(self);
        shell::refresh_status(self);
        recorder::refresh_list(self);
        explorer::refresh_tree(self);
    }

    /// Periodic work: worker progress, device events, outside changes.
    fn tick(&self) {
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
        crate::appearance::tick(self);
        if !self.state.borrow().config.headless {
            update::poll(self);
        }
        if devices::poll(self) {
            need_refresh = true;
        }
        // Changes made through other connections (MCP server, od-cli).
        let changed = self.state.borrow().lib.external_change().unwrap_or(false);
        if changed || need_refresh {
            self.refresh_library();
        }
    }
}
