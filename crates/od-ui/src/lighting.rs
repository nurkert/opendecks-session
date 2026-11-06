//! Lighting runner (SPEC §10.5): feeds the DMX show from what the decks play.
//!
//! The show runs on its own thread at 40 frames per second, so DMX timing
//! does not depend on UI repaints. The UI thread only publishes what it
//! knows about the loaded tracks ([`TrackLight`]) and the latest engine
//! state; [`deck_music`] turns both into the show's input.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use od_core::{MusicalKey, Phrase, PhraseMood};
use od_engine::{DeckState, EngineState, MAX_DECKS};
use od_lighting::{DeckMusic, Lighting, Look, Momentary, Patch, Show, ShowSettings, Universe, hue_for_key};

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
}

enum Msg {
    Engine(Box<EngineState>),
    Track(usize, Option<TrackLight>),
    Patch(Patch),
    Settings(ShowSettings),
    Momentary(Momentary),
    Output(u16, od_lighting::OutputKind),
    ClearOutputs,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_core::{PhraseKind, TrackId};

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
