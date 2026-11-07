//! Microphone / aux input (SPEC §10.2): opens the capture device at the
//! engine's sample rate, hands its ring to the engine and drives the MIC
//! controls in the global bar.

use std::rc::Rc;

use od_audio_io::{AudioInput, Backend, InputConfig};
use od_engine::{Command, EngineState, InputCommand};
use slint::ComponentHandle;

use crate::app::{App, handler};
use crate::engine;

#[derive(Default)]
pub struct MicRuntime {
    input: Option<AudioInput>,
    /// Chosen input device (`None` = system default).
    device: Option<String>,
    on: bool,
    talkover: bool,
    level: f32,
}

/// Input-capable devices of the default backend.
fn input_devices() -> Vec<String> {
    od_audio_io::list_devices(Backend::Default)
        .map(|d| {
            d.into_iter()
                .filter(|d| d.max_input_channels > 0)
                .map(|d| d.name)
                .collect()
        })
        .unwrap_or_default()
}

/// The device after `current` in `list` (`None` = default comes first).
pub fn next_device(list: &[String], current: Option<&str>) -> Option<String> {
    match current.and_then(|c| list.iter().position(|d| d == c)) {
        None => list.first().cloned(),
        Some(i) if i + 1 < list.len() => Some(list[i + 1].clone()),
        Some(_) => None,
    }
}

fn input(app: &App, c: InputCommand) {
    engine::send(app, Command::Input(c));
}

/// Opens the capture stream (if not open) and attaches it to the engine.
fn open(app: &App) -> Result<(), String> {
    if app.mic.borrow().input.is_some() {
        return Ok(());
    }
    if app.state.borrow().config.null_audio || !engine::ensure_engine(app) {
        return Err(od_i18n::tr("Audio is not running").into());
    }
    let rate = app.engine.borrow().sample_rate();
    let device = app.mic.borrow().device.clone();
    let cfg = InputConfig {
        device: device.clone(),
        sample_rate: Some(rate),
        channels: Some(2),
        ring_seconds: Some(1),
        ..Default::default()
    };
    let cfg_mono = InputConfig {
        channels: Some(1),
        ..cfg.clone()
    };
    let (inp, ring) = AudioInput::start(&cfg)
        .or_else(|_| AudioInput::start(&cfg_mono))
        .map_err(|e| e.to_string())?;
    input(app, InputCommand::Attach(ring, inp.channels));
    app.mic.borrow_mut().input = Some(inp);
    Ok(())
}

fn close(app: &App) {
    input(app, InputCommand::Detach);
    app.mic.borrow_mut().input = None;
}

fn refresh(app: &App) {
    let g = app.ui.global::<crate::Mic>();
    let m = app.mic.borrow();
    g.set_on(m.on);
    g.set_talkover(m.talkover);
    g.set_level(m.level);
    g.set_device(
        m.input
            .as_ref()
            .map(|i| i.device_name.clone())
            .or_else(|| m.device.clone())
            .unwrap_or_default()
            .into(),
    );
}

pub(crate) fn wire(app: &Rc<App>) {
    app.mic.borrow_mut().level = 0.8;
    let g = app.ui.global::<crate::Mic>();
    g.on_toggle(handler!(app, |a| {
        let on = !a.mic.borrow().on;
        let g = a.ui.global::<crate::Mic>();
        if on {
            match open(&a) {
                Ok(()) => g.set_error("".into()),
                Err(e) => {
                    g.set_error(e.clone().into());
                    a.status(format!("Microphone: {e}"));
                    return;
                }
            }
            let level = a.mic.borrow().level;
            input(&a, InputCommand::SetLevel(level));
        }
        a.mic.borrow_mut().on = on;
        input(&a, InputCommand::SetOn(on));
        refresh(&a);
    }));
    g.on_set_level(handler!(app, |a, v: f32| {
        a.mic.borrow_mut().level = v;
        input(&a, InputCommand::SetLevel(v));
    }));
    g.on_toggle_talkover(handler!(app, |a| {
        let t = !a.mic.borrow().talkover;
        a.mic.borrow_mut().talkover = t;
        input(&a, InputCommand::SetTalkover(t));
        refresh(&a);
    }));
    g.on_next_device(handler!(app, |a| {
        let list = input_devices();
        let next = next_device(&list, a.mic.borrow().device.as_deref());
        let was_open = a.mic.borrow().input.is_some();
        a.mic.borrow_mut().device = next.clone();
        a.status(format!(
            "Microphone input: {}",
            next.as_deref().unwrap_or("system default")
        ));
        if was_open {
            close(&a);
            if let Err(e) = open(&a) {
                a.ui.global::<crate::Mic>().set_error(e.into());
            }
        }
        refresh(&a);
    }));
    refresh(app);
}

/// Per engine frame: meter and ducking state.
pub(crate) fn frame(app: &App, state: &EngineState) {
    let g = app.ui.global::<crate::Mic>();
    let peak = state.input.meter.peak;
    if (g.get_peak() - peak).abs() > 0.005 {
        g.set_peak(peak);
    }
    if g.get_ducking() != state.input.ducking {
        g.set_ducking(state.input.ducking);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_button_cycles_through_inputs_and_default() {
        let list = vec!["USB Mic".to_string(), "Line In".to_string()];
        assert_eq!(next_device(&list, None).as_deref(), Some("USB Mic"));
        assert_eq!(
            next_device(&list, Some("USB Mic")).as_deref(),
            Some("Line In")
        );
        assert_eq!(next_device(&list, Some("Line In")), None);
        assert_eq!(next_device(&list, Some("gone")).as_deref(), Some("USB Mic"));
        assert_eq!(next_device(&[], None), None);
    }
}
