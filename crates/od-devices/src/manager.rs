//! Background service that watches volumes and keeps registered sticks in
//! sync automatically.
//!
//! The manager owns its own library connection and runs on its own thread.
//! It polls the volume list and the library change log; when a connected
//! stick is out of date and the library has been quiet for the debounce
//! interval, it syncs the stick in the background.

use crate::sync::{FormatWriter, NativeWriter, Progress, SyncOptions, SyncReport, sync_device};
use crate::volumes::{self, Volume};
use crossbeam_channel::{Receiver, Sender, select, tick, unbounded};
use od_core::DeviceId;
use od_library::{Device, DeviceFormats, Library, NodeId};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum DeviceState {
    Disconnected,
    InSync,
    OutOfDate { pending: usize },
    Syncing(Progress),
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceStatus {
    pub device: Device,
    pub volume: Option<Volume>,
    pub state: DeviceState,
}

#[derive(Clone, Debug)]
pub enum Event {
    /// Current status of every registered device.
    Status(Vec<DeviceStatus>),
    /// Mounted removable volumes that are not registered.
    UnregisteredVolumes(Vec<Volume>),
    SyncFinished { device: DeviceId, report: SyncReport },
    SyncFailed { device: DeviceId, error: String },
    Ejected { device: DeviceId },
    EjectFailed { device: DeviceId, error: String },
    Registered { device: DeviceId },
    /// Changes made on the stick by players (cues, history) to review.
    Proposals { device: DeviceId, proposals: Vec<crate::pioneer::Proposal> },
}

#[derive(Clone, Debug)]
pub enum Command {
    /// Registers the volume at `mount_point` as a device with the given
    /// content and syncs it.
    Register { mount_point: PathBuf, name: String, formats: DeviceFormats, nodes: Vec<NodeId> },
    SyncNow(DeviceId),
    /// Rewrites everything on the stick.
    Rebuild(DeviceId),
    Eject(DeviceId),
    SetAutoSync(bool),
    /// Re-read volumes and states now.
    Refresh,
    Shutdown,
}

/// Produces the format writers for a device's enabled formats.
pub type WriterFactory = Arc<dyn Fn(&DeviceFormats) -> Vec<Box<dyn FormatWriter + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct ManagerConfig {
    pub library_path: PathBuf,
    pub poll_interval: Duration,
    /// Quiet time after the last library change before an automatic sync.
    pub debounce: Duration,
    pub auto_sync: bool,
    pub writers: WriterFactory,
}

impl ManagerConfig {
    pub fn new(library_path: PathBuf) -> Self {
        Self {
            library_path,
            poll_interval: Duration::from_millis(1000),
            debounce: Duration::from_secs(2),
            auto_sync: true,
            writers: Arc::new(|f: &DeviceFormats| {
                let mut w: Vec<Box<dyn FormatWriter + Send>> = Vec::new();
                if f.opendecks {
                    w.push(Box::new(NativeWriter));
                }
                if f.pioneer {
                    w.push(Box::new(crate::pioneer::PioneerWriter::default()));
                }
                w
            }),
        }
    }
}

/// Handle to the background device service.
pub struct DeviceManager {
    commands: Sender<Command>,
    events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl DeviceManager {
    pub fn start(config: ManagerConfig) -> od_library::Result<Self> {
        let library = Library::open(&config.library_path)?;
        let (cmd_tx, cmd_rx) = unbounded();
        let (ev_tx, ev_rx) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            library,
            auto_sync: config.auto_sync,
            config,
            events: ev_tx,
            cancel: cancel.clone(),
            last_seq: -1,
            last_change: Instant::now(),
            errors: Default::default(),
            last_status: Vec::new(),
            last_unregistered: Vec::new(),
        };
        let thread = std::thread::Builder::new()
            .name("od-devices".into())
            .spawn(move || worker.run(cmd_rx))
            .map_err(od_library::LibraryError::Io)?;
        Ok(Self { commands: cmd_tx, events: ev_rx, cancel, thread: Some(thread) })
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Cancels a running sync (the stick stays consistent; the next sync is
    /// a full one).
    pub fn cancel_sync(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for DeviceManager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.commands.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Worker {
    library: Library,
    config: ManagerConfig,
    auto_sync: bool,
    events: Sender<Event>,
    cancel: Arc<AtomicBool>,
    last_seq: i64,
    last_change: Instant,
    errors: std::collections::HashMap<DeviceId, String>,
    last_status: Vec<DeviceStatus>,
    last_unregistered: Vec<Volume>,
}

fn new_device_uuid(mount_point: &Path) -> String {
    let mut h = blake3::Hasher::new();
    h.update(mount_point.as_os_str().as_encoded_bytes());
    h.update(&std::process::id().to_le_bytes());
    h.update(
        &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    h.finalize().to_hex()[..32].to_string()
}

impl Worker {
    fn run(mut self, commands: Receiver<Command>) {
        let ticker = tick(self.config.poll_interval);
        self.poll();
        loop {
            select! {
                recv(commands) -> cmd => match cmd {
                    Ok(Command::Shutdown) | Err(_) => break,
                    Ok(cmd) => self.handle(cmd),
                },
                recv(ticker) -> _ => self.poll(),
            }
        }
    }

    fn emit(&self, e: Event) {
        let _ = self.events.send(e);
    }

    /// Registered devices joined with mounted volumes.
    fn connected(&self) -> (Vec<(Device, Option<Volume>)>, Vec<Volume>) {
        let devices = self.library.devices().unwrap_or_default();
        let mut joined: Vec<(Device, Option<Volume>)> = devices.into_iter().map(|d| (d, None)).collect();
        let mut unregistered = Vec::new();
        for v in volumes::list() {
            let marker = od_format_native::read_marker(&v.mount_point).ok().flatten();
            match marker.and_then(|m| joined.iter_mut().find(|(d, _)| d.volume_uuid == m.device_uuid)) {
                Some(slot) => slot.1 = Some(v),
                None => unregistered.push(v),
            }
        }
        (joined, unregistered)
    }

    fn poll(&mut self) {
        let seq = self.library.change_seq().unwrap_or(0);
        if seq != self.last_seq {
            self.last_seq = seq;
            self.last_change = Instant::now();
        }
        let quiet = self.last_change.elapsed() >= self.config.debounce;
        let (joined, unregistered) = self.connected();
        let mut statuses = Vec::with_capacity(joined.len());
        let mut to_sync = Vec::new();
        for (device, volume) in joined {
            let state = match &volume {
                None => DeviceState::Disconnected,
                Some(_) => match self.errors.get(&device.id) {
                    Some(e) => DeviceState::Error(e.clone()),
                    None => match self.library.pending_changes(device.id) {
                        Ok(p) if p.is_empty() => DeviceState::InSync,
                        Ok(p) => {
                            if self.auto_sync && quiet {
                                to_sync.push(device.id);
                            }
                            DeviceState::OutOfDate { pending: p.count }
                        }
                        Err(e) => DeviceState::Error(e.to_string()),
                    },
                },
            };
            if volume.is_none() {
                // Errors are cleared when the stick is unplugged.
                self.errors.remove(&device.id);
            }
            statuses.push(DeviceStatus { device, volume, state });
        }
        if statuses != self.last_status {
            self.last_status = statuses.clone();
            self.emit(Event::Status(statuses));
        }
        if unregistered != self.last_unregistered {
            self.last_unregistered = unregistered.clone();
            self.emit(Event::UnregisteredVolumes(unregistered));
        }
        for id in to_sync {
            self.sync(id, false);
        }
    }

    fn volume_of(&self, id: DeviceId) -> Option<Volume> {
        self.connected().0.into_iter().find(|(d, _)| d.id == id).and_then(|(_, v)| v)
    }

    fn set_state(&mut self, id: DeviceId, state: DeviceState) {
        if let Some(s) = self.last_status.iter_mut().find(|s| s.device.id == id) {
            s.state = state;
        }
        self.emit(Event::Status(self.last_status.clone()));
    }

    fn sync(&mut self, id: DeviceId, full: bool) {
        let Some(volume) = self.volume_of(id) else { return };
        let Ok(device) = self.library.device(id) else { return };
        self.cancel.store(false, Ordering::Relaxed);
        let mut writers = (self.config.writers)(&device.formats);
        let events = self.events.clone();
        let mut status = self.last_status.clone();
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let result = sync_device(
            &mut self.library,
            id,
            &volume.mount_point,
            &mut writers,
            &SyncOptions { force_full: full },
            &self.cancel,
            |p| {
                // Rate-limit progress events to ~10 per second.
                if last_emit.elapsed() < Duration::from_millis(100) && matches!(p, Progress::Copying { .. }) {
                    return;
                }
                last_emit = Instant::now();
                if let Some(s) = status.iter_mut().find(|s| s.device.id == id) {
                    s.state = DeviceState::Syncing(p);
                }
                let _ = events.send(Event::Status(status.clone()));
            },
        );
        match result {
            Ok(report) => {
                self.errors.remove(&id);
                tracing::info!(device = %id, copied = report.copied, updated = report.updated, "sync finished");
                if !report.proposals.is_empty() {
                    self.emit(Event::Proposals { device: id, proposals: report.proposals.clone() });
                }
                self.emit(Event::SyncFinished { device: id, report });
                self.set_state(id, DeviceState::InSync);
            }
            Err(e) => {
                let msg = e.to_string();
                tracing::warn!(device = %id, error = %msg, "sync failed");
                let _ = self.library.mark_device_error(id, &msg);
                if !matches!(e, crate::SyncError::Cancelled) {
                    self.errors.insert(id, msg.clone());
                }
                self.emit(Event::SyncFailed { device: id, error: msg.clone() });
                self.set_state(id, DeviceState::Error(msg));
            }
        }
        // Force the next poll to re-evaluate.
        self.last_status.clear();
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Register { mount_point, name, formats, nodes } => {
                let existing = od_format_native::read_marker(&mount_point).ok().flatten();
                let uuid = existing.map(|m| m.device_uuid).unwrap_or_else(|| new_device_uuid(&mount_point));
                let marker = od_format_native::DeviceMarker {
                    format_version: od_format_native::FORMAT_VERSION,
                    device_uuid: uuid.clone(),
                    name: name.clone(),
                    created_by: format!("OpenDecks Session {}", env!("CARGO_PKG_VERSION")),
                };
                let result = od_format_native::write_marker(&mount_point, &marker)
                    .map_err(|e| e.to_string())
                    .and_then(|_| self.library.register_device(&uuid, &name, formats).map_err(|e| e.to_string()))
                    .and_then(|id| self.library.set_device_nodes(id, &nodes).map(|_| id).map_err(|e| e.to_string()));
                match result {
                    Ok(id) => {
                        self.emit(Event::Registered { device: id });
                        self.last_status.clear();
                        self.poll();
                        self.sync(id, true);
                    }
                    Err(e) => tracing::warn!(error = %e, "registering device failed"),
                }
            }
            Command::SyncNow(id) => {
                self.errors.remove(&id);
                self.sync(id, false);
            }
            Command::Rebuild(id) => {
                self.errors.remove(&id);
                let _ = self.library.reset_device_sync(id);
                self.sync(id, true);
            }
            Command::Eject(id) => match self.volume_of(id) {
                Some(v) => match volumes::eject(&v) {
                    Ok(()) => {
                        self.emit(Event::Ejected { device: id });
                        self.last_status.clear();
                        self.poll();
                    }
                    Err(e) => self.emit(Event::EjectFailed { device: id, error: e.to_string() }),
                },
                None => self.emit(Event::Ejected { device: id }),
            },
            Command::SetAutoSync(on) => self.auto_sync = on,
            Command::Refresh => {
                self.last_status.clear();
                self.last_unregistered.clear();
                self.poll();
            }
            Command::Shutdown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{Cue, TrackTags};
    use od_library::{NewTrack, NodeKind};

    // Uses the OPENDECKS_VOLUMES_DIR test hook; the only test touching the
    // environment in this crate.
    #[test]
    fn registers_and_auto_syncs() {
        let dir = tempfile::tempdir().unwrap();
        let volumes_dir = dir.path().join("volumes");
        let stick = volumes_dir.join("STICK");
        std::fs::create_dir_all(&stick).unwrap();
        // SAFETY: tests in this crate do not read this variable concurrently
        // except through this manager.
        unsafe { std::env::set_var("OPENDECKS_VOLUMES_DIR", &volumes_dir) };

        let lib_path = dir.path().join("library.sqlite");
        let mut lib = Library::open(&lib_path).unwrap();
        let file = dir.path().join("a.mp3");
        std::fs::write(&file, vec![1u8; 4096]).unwrap();
        let a = lib
            .add_track(&NewTrack {
                content_hash: od_decode::content_hash(&file).unwrap(),
                file_size: 4096,
                path: file,
                format: None,
                duration: 1.0,
                sample_rate: 44_100,
                channels: 2,
                bit_rate: None,
                bits_per_sample: None,
                tags: TrackTags { title: "A".into(), ..Default::default() },
                artwork: None,
            })
            .unwrap();
        let pl = lib.create_node(None, NodeKind::Playlist, "P").unwrap();
        lib.add_to_node(pl, &[a], None).unwrap();

        let mut cfg = ManagerConfig::new(lib_path);
        cfg.poll_interval = Duration::from_millis(50);
        cfg.debounce = Duration::from_millis(100);
        let mgr = DeviceManager::start(cfg).unwrap();
        let wait_for = |pred: &dyn Fn(&Event) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                if let Ok(e) = mgr.events().recv_timeout(Duration::from_millis(100)) {
                    if pred(&e) {
                        return e;
                    }
                }
            }
            panic!("timed out waiting for event");
        };
        wait_for(&|e| matches!(e, Event::UnregisteredVolumes(v) if v.len() == 1));
        mgr.send(Command::Register {
            mount_point: stick.clone(),
            name: "Stick".into(),
            formats: DeviceFormats::default(),
            nodes: vec![pl],
        });
        wait_for(&|e| matches!(e, Event::SyncFinished { report, .. } if report.copied == 1));

        // Edit a cue in the app's own connection; the manager picks it up.
        lib.set_cues(a, vec![Cue::hot(1, 4.0)]).unwrap();
        wait_for(&|e| matches!(e, Event::SyncFinished { report, .. } if report.updated == 1 && report.copied == 0));
        let s = od_format_native::Stick::open(&stick).unwrap().unwrap();
        assert_eq!(s.library.cues(a).unwrap()[0].start, 4.0);
    }
}
