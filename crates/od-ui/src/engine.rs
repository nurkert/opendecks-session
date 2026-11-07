//! Audio engine lifecycle and the frame tick that projects engine state into
//! the deck and mixer view models.

use crate::app::App;
use crate::{controller, decks, explorer, fx, mixer, player, recorder, sampler};
use od_engine::{Command, DeckCommand, Engine, EngineConfig, EngineState, Profile};
use slint::ComponentHandle;
use std::rc::Rc;
use std::time::{Duration, Instant};

pub(crate) struct EngineHost {
    pub engine: Option<Engine>,
    host: Option<od_audio_io::AudioHost>,
    /// Audio rendered into nothing (headless tests, no audio device wanted).
    null_host: Option<NullHost>,
    /// Device line or the reason audio is not running.
    pub message: String,
    timer: slint::Timer,
    last_feedback: Instant,
}

impl Default for EngineHost {
    fn default() -> Self {
        Self {
            engine: None,
            host: None,
            null_host: None,
            message: od_i18n::tr("Audio starts when you load a track").into(),
            timer: slint::Timer::default(),
            last_feedback: Instant::now(),
        }
    }
}

impl EngineHost {
    pub fn sample_rate(&self) -> u32 {
        self.engine
            .as_ref()
            .map(|e| e.config().sample_rate)
            .unwrap_or(48_000)
    }
}

/// The current engine state, if audio runs.
pub(crate) fn snapshot(app: &App) -> Option<EngineState> {
    app.engine.borrow_mut().engine.as_mut().map(|e| *e.state())
}

/// Starts audio on first use, then applies the deck and mixer settings
/// (Preferences) to the new engine.
pub(crate) fn ensure_engine(app: &App) -> bool {
    if app.engine.borrow().engine.is_some() {
        return true;
    }
    let started = start_engine(app);
    if started {
        crate::preferences::apply_engine_settings(app);
    }
    started
}

/// Starts audio. Prefers the controller's audio interface.
fn start_engine(app: &App) -> bool {
    let lite = app.state.borrow().config.lite;
    if app.state.borrow().config.null_audio {
        start_null(app, lite);
        return true;
    }
    let audio = app.state.borrow().settings.audio.clone();
    let mut cfg = audio_config(&audio, lite);
    if cfg.device.is_none() {
        cfg.device = od_audio_io::list_devices(od_audio_io::Backend::Default)
            .ok()
            .and_then(|devs| {
                devs.into_iter()
                    .find(|d| d.name.contains("DDJ") || d.name.contains("FLX"))
                    .map(|d| d.name)
            });
    }
    let negotiated = match od_audio_io::AudioHost::negotiate(&cfg) {
        Ok(n) => n,
        Err(e) => {
            app.engine.borrow_mut().message = format!("No audio output: {e}");
            return false;
        }
    };
    cfg.sample_rate = Some(negotiated.sample_rate);
    let (mut engine, processor) = Engine::new(EngineConfig {
        sample_rate: negotiated.sample_rate,
        max_block: 2048,
        decks: decks::DECKS,
        profile: if lite { Profile::Lite } else { Profile::Full },
    });
    match od_audio_io::AudioHost::start(&cfg, processor) {
        Ok(host) => {
            if let Some(tap) = engine.take_record_tap() {
                recorder::attach(app, tap, negotiated.sample_rate);
            }
            let mut h = app.engine.borrow_mut();
            h.message = format!(
                "{} · {} Hz{}",
                negotiated.device_name,
                negotiated.sample_rate,
                if negotiated.routing.headphones.is_some() {
                    " · headphones on 3/4"
                } else {
                    ""
                }
            );
            h.engine = Some(engine);
            h.host = Some(host);
            true
        }
        Err(e) => {
            app.engine.borrow_mut().message = format!("Audio could not start: {e}");
            false
        }
    }
}

/// A stand-in for the audio device: a thread that runs the engine processor
/// in real time and discards the output. Everything else (decks, mixer,
/// recorder tap) behaves as with a device, so the UI can be driven
/// headlessly (UI tests, CI machines without audio).
struct NullHost {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for NullHost {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

const NULL_RATE: u32 = 48_000;
const NULL_BLOCK: usize = 480;

fn start_null(app: &App, lite: bool) {
    let (mut engine, mut processor) = Engine::new(EngineConfig {
        sample_rate: NULL_RATE,
        max_block: 2048,
        decks: decks::DECKS,
        profile: if lite { Profile::Lite } else { Profile::Full },
    });
    if let Some(tap) = engine.take_record_tap() {
        recorder::attach(app, tap, NULL_RATE);
    }
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("od-null-audio".into())
        .spawn(move || {
            let mut master = vec![0.0f32; NULL_BLOCK * 2];
            let mut phones = vec![0.0f32; NULL_BLOCK * 2];
            let period = Duration::from_secs_f64(NULL_BLOCK as f64 / NULL_RATE as f64);
            let mut next = Instant::now();
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                processor.process(
                    NULL_BLOCK,
                    &mut od_engine::Outputs {
                        master: &mut master,
                        headphones: Some(&mut phones),
                        booth: None,
                    },
                );
                next += period;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    next = now;
                }
            }
        })
        .ok();
    let mut h = app.engine.borrow_mut();
    h.message = od_i18n::tr("No audio output (headless)").into();
    h.engine = Some(engine);
    h.null_host = Some(NullHost { stop, thread });
}

/// The audio configuration from Preferences > Audio (device `None` =
/// automatic).
pub(crate) fn audio_config(
    a: &crate::settings::AudioSettings,
    lite: bool,
) -> od_audio_io::AudioConfig {
    let routing = match a.headphones.as_str() {
        "34" => Some(od_audio_io::OutputRouting {
            master: [0, 1],
            headphones: Some([2, 3]),
        }),
        "off" => Some(od_audio_io::OutputRouting {
            master: [0, 1],
            headphones: None,
        }),
        _ => None,
    };
    od_audio_io::AudioConfig {
        device: (!a.device.is_empty()).then(|| a.device.clone()),
        buffer_frames: Some(if a.buffer > 0 {
            a.buffer
        } else if lite {
            256
        } else {
            128
        }),
        sample_rate: (a.sample_rate > 0).then_some(a.sample_rate),
        routing,
        ..Default::default()
    }
}

pub(crate) fn send(app: &App, cmd: Command) {
    if let Some(e) = app.engine.borrow_mut().engine.as_mut() {
        if let Err(err) = e.send(cmd) {
            tracing::warn!(error = ?err, "engine queue full");
        }
    }
}

pub(crate) fn deck_cmd(app: &App, deck: usize, cmd: DeckCommand) {
    send(app, Command::Deck(deck, cmd));
}

/// Shows the engine message (device line, loading, errors).
pub(crate) fn show_message(app: &App) {
    let msg = app.engine.borrow().message.clone();
    app.ui.set_engine_message(msg.into());
}

fn frame(app: &App) {
    let scale = app.ui.window().scale_factor();
    let small = (app.ui.window().size().height as f32 / scale) < 620.0;
    if app.ui.get_small_screen() != small {
        app.ui.set_small_screen(small);
    }
    let narrow = (app.ui.window().size().width as f32 / scale) < 1500.0;
    if app.ui.get_narrow_screen() != narrow {
        app.ui.set_narrow_screen(narrow);
    }
    // Touch layout: the kiosk, or a window as short as the kiosk screen.
    let touch =
        app.state.borrow().config.kiosk || (app.ui.window().size().height as f32 / scale) <= 480.0;
    if app.ui.global::<crate::Theme>().get_touch() != touch {
        app.ui.global::<crate::Theme>().set_touch(touch);
    }
    let short = (app.ui.window().size().height as f32 / scale) < 860.0;
    if app.ui.get_short_screen() != short {
        app.ui.set_short_screen(short);
    }
    decks::receive_prepared(app);
    decks::poll_stems(app);
    explorer::poll(app);
    crate::browser::poll_artworks(app);
    crate::lighting::poll_remote(app);
    controller::poll(app);
    sampler::poll(app);

    let (state, events) = match app.engine.borrow_mut().engine.as_mut() {
        Some(e) => {
            e.collect_garbage();
            (Some(*e.state()), e.events().collect::<Vec<_>>())
        }
        None => (None, Vec::new()),
    };
    decks::persist_cue_events(app, &events);
    crate::lighting::frame(app, state.as_ref());
    top_bar(app, state.as_ref());
    if app.ui.get_mode() == "prepare" {
        player::project_controls(app, state.as_ref());
    }
    let Some(state) = state else {
        // Before audio starts: empty deck plates, so the layout is complete.
        if app.ui.get_mode() == "perform" {
            decks::show_empty(app);
        }
        return;
    };
    player::follow_preview(app, &state);
    recorder::frame(app, &state);
    if app.ui.get_mode() != "perform" {
        return;
    }
    decks::project(app, &state);
    crate::cdj::frame(app, &state);
    crate::mic::frame(app, &state);
    mixer::project(app, &state);
    fx::project(app, &state);
    sampler::project(app, &state);
    app.ui.set_engine_ready(true);
    if let Some(h) = app.engine.borrow().host.as_ref() {
        let s = h.stats();
        if s.xruns > 0 || s.overruns > 0 {
            app.ui
                .set_engine_message(format!("Audio dropouts: {}", s.xruns + s.overruns).into());
        }
    }
}

thread_local! {
    /// Minute (since the epoch) the top bar clock shows.
    static CLOCK_MINUTE: std::cell::Cell<i64> = const { std::cell::Cell::new(-1) };
}

/// Top bar: controller state, master meter and the wall clock (local time
/// from SQLite, refreshed once a minute).
fn top_bar(app: &App, state: Option<&od_engine::EngineState>) {
    let midi = app.controller.borrow().name.clone().unwrap_or_default();
    if app.ui.get_midi_device() != midi.as_str() {
        app.ui.set_midi_device(midi.into());
    }
    let (meter, clip) = state
        .map(|s| {
            (
                s.master_meter.rms.max(s.master_meter.peak * 0.8).min(1.0),
                s.master_meter.clipped,
            )
        })
        .unwrap_or((0.0, false));
    app.ui.set_master_meter(meter);
    app.ui.set_master_clip(clip);
    let minute = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 60)
        .unwrap_or(0);
    if CLOCK_MINUTE.with(|c| c.replace(minute)) != minute || app.ui.get_clock().is_empty() {
        let now: Option<String> = app
            .state
            .borrow()
            .lib
            .conn()
            .query_row("SELECT strftime('%H:%M', 'now', 'localtime')", [], |r| {
                r.get(0)
            })
            .ok();
        app.ui.set_clock(now.unwrap_or_default().into());
    }
}

/// Starts the frame timer (60 Hz, 30 Hz in Lite) and shows the engine line.
pub(crate) fn start(app: &Rc<App>) {
    let lite = app.state.borrow().config.lite;
    let period = Duration::from_millis(if lite { 33 } else { 16 });
    let w = Rc::downgrade(app);
    app.engine
        .borrow()
        .timer
        .start(slint::TimerMode::Repeated, period, move || {
            let Some(app) = w.upgrade() else { return };
            frame(&app);
            let due = app.engine.borrow().last_feedback.elapsed() >= Duration::from_millis(40);
            if due {
                app.engine.borrow_mut().last_feedback = Instant::now();
                controller::feedback(&app);
            }
        });
    show_message(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_settings_to_config() {
        let mut a = crate::settings::AudioSettings::default();
        let c = audio_config(&a, false);
        assert_eq!(
            (c.device, c.buffer_frames, c.sample_rate, c.routing),
            (None, Some(128), None, None)
        );
        assert_eq!(audio_config(&a, true).buffer_frames, Some(256));
        a.device = "Speakers".into();
        a.buffer = 512;
        a.sample_rate = 44_100;
        a.headphones = "off".into();
        let c = audio_config(&a, true);
        assert_eq!(c.device.as_deref(), Some("Speakers"));
        assert_eq!(c.buffer_frames, Some(512));
        assert_eq!(c.sample_rate, Some(44_100));
        assert_eq!(c.routing.unwrap().headphones, None);
    }
}
