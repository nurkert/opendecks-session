//! Real MIDI I/O through `midir` (CoreMIDI on macOS, ALSA on Linux).
//!
//! [`ControllerManager`] owns a controller thread. It periodically rescans
//! MIDI ports (hot-plug), connects ports matching a mapping, turns incoming
//! messages into [`ControllerEvent::Action`]s, and renders feedback pushed
//! with [`ControllerManager::send_feedback`] into diffed, rate-limited MIDI.
//! All midir objects live on the controller thread.

use crate::action::ControlAction;
use crate::feedback::ControlFeedback;
use crate::learn::LearnSession;
use crate::mapping::Mapping;
use crate::monitor::{Direction, MidiMonitor};
use crate::session::ControllerSession;
use crossbeam_channel::{Receiver, Sender};
use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CLIENT: &str = "OpenDecks Session";

/// MIDI port names currently present.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PortList {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

pub fn list_ports() -> Result<PortList, String> {
    let mi = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    let mo = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
    Ok(PortList {
        inputs: mi
            .ports()
            .iter()
            .filter_map(|p| mi.port_name(p).ok())
            .collect(),
        outputs: mo
            .ports()
            .iter()
            .filter_map(|p| mo.port_name(p).ok())
            .collect(),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControllerEvent {
    Connected {
        device: String,
        port: String,
        usb: Option<(u16, u16)>,
    },
    Disconnected {
        device: String,
        port: String,
    },
    /// `timestamp_us` is the backend timestamp of the triggering message.
    Action {
        device: String,
        action: ControlAction,
        timestamp_us: u64,
    },
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ManagerConfig {
    pub rescan_interval: Duration,
    /// Output poll / loop period.
    pub tick: Duration,
    /// Also open input ports without a mapping (for the monitor and learn).
    pub monitor_unmapped_ports: bool,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            rescan_interval: Duration::from_secs(2),
            tick: Duration::from_millis(5),
            monitor_unmapped_ports: false,
        }
    }
}

struct RawIn {
    port: String,
    stamp: u64,
    bytes: Vec<u8>,
}

struct Connected {
    _input: MidiInputConnection<()>,
    output: Option<MidiOutputConnection>,
    session: Option<ControllerSession>,
    device: String,
}

pub struct ControllerManager {
    events: Receiver<ControllerEvent>,
    feedback: Sender<(Option<String>, ControlFeedback)>,
    monitor: MidiMonitor,
    learn: Arc<Mutex<Option<LearnSession>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControllerManager {
    pub fn start(mappings: Vec<Arc<Mapping>>, config: ManagerConfig) -> Self {
        let (ev_tx, ev_rx) = crossbeam_channel::unbounded();
        let (fb_tx, fb_rx) = crossbeam_channel::unbounded();
        let monitor = MidiMonitor::default();
        let learn = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let monitor = monitor.clone();
            let learn = learn.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("od-controller".into())
                .spawn(move || run(mappings, config, ev_tx, fb_rx, monitor, learn, stop))
                .expect("spawn controller thread")
        };
        Self {
            events: ev_rx,
            feedback: fb_tx,
            monitor,
            learn,
            stop,
            thread: Some(thread),
        }
    }

    pub fn events(&self) -> &Receiver<ControllerEvent> {
        &self.events
    }

    /// Pushes feedback to one device (by mapping id) or to all (`None`).
    pub fn send_feedback(&self, device: Option<&str>, fb: ControlFeedback) {
        let _ = self.feedback.send((device.map(str::to_string), fb));
    }

    pub fn monitor(&self) -> &MidiMonitor {
        &self.monitor
    }

    /// Starts a MIDI learn session; incoming messages are fed to it.
    pub fn start_learn(&self) {
        *self.learn.lock().unwrap() = Some(LearnSession::new());
    }

    /// Ends the learn session and returns it for [`LearnSession::suggest`].
    pub fn finish_learn(&self) -> Option<LearnSession> {
        self.learn.lock().unwrap().take()
    }
}

impl Drop for ControllerManager {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(
    mappings: Vec<Arc<Mapping>>,
    config: ManagerConfig,
    events: Sender<ControllerEvent>,
    feedback: Receiver<(Option<String>, ControlFeedback)>,
    monitor: MidiMonitor,
    learn: Arc<Mutex<Option<LearnSession>>>,
    stop: Arc<AtomicBool>,
) {
    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<RawIn>();
    let mut conns: HashMap<String, Connected> = HashMap::new();
    let mut last_scan: Option<Instant> = None;

    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if last_scan.is_none_or(|t| now.duration_since(t) >= config.rescan_interval) {
            last_scan = Some(now);
            rescan(&mappings, &config, &mut conns, &raw_tx, &events, &monitor);
        }

        // Input.
        while let Ok(raw) = raw_rx.try_recv() {
            if let Some(l) = learn.lock().unwrap().as_mut() {
                l.feed(&raw.bytes);
            }
            let Some(c) = conns.get_mut(&raw.port) else {
                continue;
            };
            match c.session.as_mut() {
                Some(s) => {
                    let p = s.handle_midi(&raw.bytes);
                    monitor.record(&raw.port, Direction::In, &raw.bytes, p.hits);
                    for action in p.actions {
                        let _ = events.send(ControllerEvent::Action {
                            device: c.device.clone(),
                            action,
                            timestamp_us: raw.stamp,
                        });
                    }
                }
                None => monitor.record(&raw.port, Direction::In, &raw.bytes, Vec::new()),
            }
        }

        // Feedback.
        while let Ok((dev, fb)) = feedback.try_recv() {
            for c in conns.values_mut() {
                if dev.as_ref().is_none_or(|d| *d == c.device)
                    && let Some(s) = c.session.as_mut()
                {
                    s.feedback(&fb);
                }
            }
        }

        // Output.
        for (port, c) in conns.iter_mut() {
            let (Some(s), Some(out)) = (c.session.as_mut(), c.output.as_mut()) else {
                continue;
            };
            for msg in s.poll_output(now) {
                if let Err(e) = out.send(&msg) {
                    let _ = events.send(ControllerEvent::Error(format!("{port}: {e}")));
                    break;
                }
                monitor.record(port, Direction::Out, &msg, Vec::new());
            }
        }

        std::thread::sleep(config.tick);
    }
}

fn rescan(
    mappings: &[Arc<Mapping>],
    config: &ManagerConfig,
    conns: &mut HashMap<String, Connected>,
    raw_tx: &Sender<RawIn>,
    events: &Sender<ControllerEvent>,
    monitor: &MidiMonitor,
) {
    let ports = match list_ports() {
        Ok(p) => p,
        Err(e) => {
            let _ = events.send(ControllerEvent::Error(format!(
                "MIDI port scan failed: {e}"
            )));
            return;
        }
    };
    // Unplugged ports.
    let gone: Vec<String> = conns
        .keys()
        .filter(|p| !ports.inputs.contains(p))
        .cloned()
        .collect();
    for p in gone {
        if let Some(c) = conns.remove(&p) {
            tracing::info!("controller port gone: {p}");
            if c.session.is_some() {
                let _ = events.send(ControllerEvent::Disconnected {
                    device: c.device,
                    port: p,
                });
            }
        }
    }
    // New ports.
    let usb = crate::usb::list_usb_devices();
    for name in &ports.inputs {
        if conns.contains_key(name) {
            continue;
        }
        let mapping = mappings.iter().find(|m| m.matches_port(name)).cloned();
        if mapping.is_none() && !config.monitor_unmapped_ports {
            continue;
        }
        let input = match connect_input(name, raw_tx.clone()) {
            Ok(i) => i,
            Err(e) => {
                let _ = events.send(ControllerEvent::Error(format!("open {name}: {e}")));
                continue;
            }
        };
        let (session, output, device) = match &mapping {
            Some(m) => {
                let mut session = ControllerSession::new(m.clone());
                let out_name = ports
                    .outputs
                    .iter()
                    .find(|o| *o == name)
                    .or_else(|| ports.outputs.iter().find(|o| m.matches_port(o)));
                let mut output = out_name.and_then(|o| connect_output(o).ok());
                if let Some(out) = output.as_mut() {
                    for msg in session.connect_messages() {
                        let _ = out.send(&msg);
                        monitor.record(name, Direction::Out, &msg, Vec::new());
                    }
                }
                let ids = usb
                    .iter()
                    .find(|d| m.matches_usb(d.vendor_id, d.product_id))
                    .map(|d| (d.vendor_id, d.product_id));
                let device = m.device().id.clone();
                let _ = events.send(ControllerEvent::Connected {
                    device: device.clone(),
                    port: name.clone(),
                    usb: ids,
                });
                tracing::info!("controller connected: {device} on {name}");
                (Some(session), output, device)
            }
            None => (None, None, String::new()),
        };
        conns.insert(
            name.clone(),
            Connected {
                _input: input,
                output,
                session,
                device,
            },
        );
    }
}

fn connect_input(name: &str, tx: Sender<RawIn>) -> Result<MidiInputConnection<()>, String> {
    let mut mi = MidiInput::new(CLIENT).map_err(|e| e.to_string())?;
    mi.ignore(midir::Ignore::None);
    let port = mi
        .ports()
        .into_iter()
        .find(|p| mi.port_name(p).ok().as_deref() == Some(name))
        .ok_or("port vanished")?;
    let port_name = name.to_string();
    mi.connect(
        &port,
        "opendecks-in",
        move |stamp, bytes, _| {
            let _ = tx.send(RawIn {
                port: port_name.clone(),
                stamp,
                bytes: bytes.to_vec(),
            });
        },
        (),
    )
    .map_err(|e| e.to_string())
}

fn connect_output(name: &str) -> Result<MidiOutputConnection, String> {
    let mo = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
    let port = mo
        .ports()
        .into_iter()
        .find(|p| mo.port_name(p).ok().as_deref() == Some(name))
        .ok_or("port vanished")?;
    mo.connect(&port, "opendecks-out")
        .map_err(|e| e.to_string())
}
