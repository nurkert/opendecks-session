//! Preferences ▸ Controller tools: the MIDI monitor (raw messages with what
//! the mapping made of them), MIDI learn (move a control, get a mapping
//! entry, save it to your own mapping file) and the DDJ-FLX10 utility
//! settings (written to the unit over USB, `od-controller::flx10_settings`).

use crate::app::App;
use crate::settings::Flx10Prefs;
use crossbeam_channel::Receiver;
use od_controller::flx10_settings::{
    BoostLevel, FaderStart, Flx10Setting, JogRingColor, RecOut, SlipButtonLed, TimeMode,
};
use od_controller::learn::{LearnSession, LearnedInput};
use od_controller::monitor::{Direction, MidiMonitor, MonitorEvent};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Lines the monitor keeps on screen.
pub(crate) const MONITOR_LINES: usize = 200;

/// Runtime state of the tools (part of the Preferences state).
#[derive(Default)]
pub(crate) struct MidiTools {
    /// The controller layer's monitor (tests put their own here).
    pub monitor: Option<MidiMonitor>,
    rx: Option<Receiver<MonitorEvent>>,
    pub monitoring: bool,
    pub lines: VecDeque<String>,
    pub learning: bool,
    learn: LearnSession,
    /// Port the learned control came from.
    pub learn_port: String,
    /// Index into `od_controller::templates::ACTION_NAMES`.
    pub action: usize,
    /// Deck / channel of the learned entry (1-based; 0 = none).
    pub deck: u8,
    pub suggestion: Option<LearnedInput>,
}

impl MidiTools {
    /// Starts or stops the monitor / learn stream.
    fn ensure_stream(&mut self) {
        let want = self.monitoring || self.learning;
        match (want, &self.rx, &self.monitor) {
            (true, None, Some(m)) => {
                m.set_enabled(true);
                self.rx = Some(m.subscribe());
            }
            (false, Some(_), _) => self.rx = None,
            _ => {}
        }
    }

    pub fn set_monitoring(&mut self, on: bool) {
        self.monitoring = on;
        self.ensure_stream();
    }

    pub fn start_learn(&mut self) {
        self.learn.reset();
        self.suggestion = None;
        self.learn_port.clear();
        self.learning = true;
        self.ensure_stream();
    }

    pub fn stop_learn(&mut self) {
        self.learning = false;
        self.ensure_stream();
    }

    /// Takes the new messages: monitor lines and learn input. Returns true
    /// when something changed.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.rx else { return false };
        let events: Vec<MonitorEvent> = rx.try_iter().collect();
        if events.is_empty() {
            return false;
        }
        for e in events {
            if self.monitoring {
                for l in e.to_string().lines() {
                    if self.lines.len() == MONITOR_LINES {
                        self.lines.pop_front();
                    }
                    self.lines.push_back(l.to_string());
                }
            }
            if self.learning && e.direction == Direction::In {
                self.learn.feed(&e.bytes);
                if self.learn_port.is_empty() {
                    self.learn_port = e.port.clone();
                }
                self.suggestion = self.learn.suggest();
            }
        }
        true
    }

    pub fn action_name(&self) -> &'static str {
        od_controller::templates::ACTION_NAMES
            .get(self.action)
            .copied()
            .unwrap_or("ignore")
    }

    /// The `[[input]]` entry for the learned control, if any.
    pub fn entry(&self) -> Option<String> {
        self.suggestion
            .as_ref()
            .map(|l| input_block(l, self.action_name(), (self.deck > 0).then_some(self.deck)))
    }
}

/// A short description of a learned control: "CC 0x13 / 0x33 · channel 1 ·
/// 14-bit fader".
pub(crate) fn describe(l: &LearnedInput) -> String {
    use od_controller::mapping::InputMode;
    use od_controller::midi::MsgKind;
    let kind = match l.addr.kind {
        MsgKind::Note => format!("NOTE 0x{:02X}", l.addr.number),
        MsgKind::Cc => match l.lsb {
            Some(lsb) => format!("CC 0x{:02X} / 0x{lsb:02X}", l.addr.number),
            None => format!("CC 0x{:02X}", l.addr.number),
        },
    };
    let mode = match l.mode {
        InputMode::Button => od_i18n::tr("button"),
        InputMode::Absolute => od_i18n::tr("knob / fader"),
        InputMode::Absolute14 => od_i18n::tr("14-bit fader"),
        InputMode::Relative => od_i18n::tr("encoder / jog"),
    };
    od_i18n::trf(
        "{} · channel {} · {}",
        &[&kind, &(l.addr.channel + 1), &mode],
    )
}

/// The mapping entry of a learned control, with its deck when given.
pub(crate) fn input_block(l: &LearnedInput, action: &str, deck: Option<u8>) -> String {
    let name = action.replace(['.', '_'], " ");
    let mut s = l.to_toml(action, &name);
    if let Some(d) = deck {
        s = s.replace(
            &format!("action = {action:?}"),
            &format!("deck = {d}\naction = {action:?}"),
        );
    }
    s
}

/// A new mapping file for learned controls of the device on `port`.
pub(crate) fn mapping_header(port: &str) -> String {
    let port = if port.is_empty() { "MIDI" } else { port };
    format!(
        "# Controls learned in OpenDecks Session (Preferences > Controller > MIDI learn).\n\
         schema = 1\n\n\
         [device]\n\
         id = \"user-learned\"\n\
         name = \"My controller\"\n\
         vendor = \"\"\n\
         model = {port:?}\n\
         decks = 4\n\
         sources = [\"MIDI learn\"]\n\n\
         [device.match]\n\
         port_names = [{port:?}]\n\n"
    )
}

/// Adds `entry` to the mapping file at `path` (created with a header for
/// `port` when missing) and checks that the result still loads; a file
/// that would not load is left as it was.
pub(crate) fn save_entry(path: &Path, port: &str, entry: &str) -> Result<(), String> {
    let before = std::fs::read_to_string(path).ok();
    let mut text = before.clone().unwrap_or_else(|| mapping_header(port));
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n');
    text.push_str(entry);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, &text).map_err(|e| e.to_string())?;
    if let Err(e) = od_controller::Mapping::load(path) {
        match before {
            Some(b) => {
                let _ = std::fs::write(path, b);
            }
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
        return Err(e.to_string());
    }
    Ok(())
}

/// Where learned controls go when no own mapping file is set.
pub(crate) fn default_learn_file(library: &Path) -> PathBuf {
    library
        .parent()
        .unwrap_or(Path::new("."))
        .join("mappings/learned.toml")
}

// ── DDJ-FLX10 utility settings ──────────────────────────────────────────

/// Choices offered per FLX10 setting (stored values).
pub(crate) const FADER_STARTS: [&str; 3] = ["off", "on", "on-sync"];
pub(crate) const REC_OUTS: [&str; 2] = ["with-mic", "without-mic"];
pub(crate) const TIME_MODES: [&str; 2] = ["remaining", "elapsed"];
pub(crate) const SLIP_LEDS: [&str; 2] = ["lit", "blink"];
pub(crate) const BOOSTS: [i32; 6] = [0, 3, 6, 9, 12, 15];

/// The settings to write to the unit, in the order the vendor utility uses.
pub(crate) fn flx10_settings(p: &Flx10Prefs) -> Vec<Flx10Setting> {
    let fader = match p.fader_start.as_str() {
        "off" => FaderStart::Off,
        "on-sync" => FaderStart::OnWithSync,
        _ => FaderStart::On,
    };
    let ring = match p.jog_ring {
        2 => JogRingColor::Mode2,
        3 => JogRingColor::Mode3,
        _ => JogRingColor::Mode1,
    };
    let boost = match p.boost_db {
        15 => BoostLevel::Plus15dB,
        12 => BoostLevel::Plus12dB,
        9 => BoostLevel::Plus9dB,
        6 => BoostLevel::Plus6dB,
        3 => BoostLevel::Plus3dB,
        _ => BoostLevel::ZeroDb,
    };
    vec![
        Flx10Setting::SlipModeFlashing(p.slip_flashing),
        Flx10Setting::SlipButtonLed(if p.slip_led == "blink" {
            SlipButtonLed::Blink
        } else {
            SlipButtonLed::Lit
        }),
        Flx10Setting::FaderStart(fader),
        Flx10Setting::JogRingColor(ring),
        Flx10Setting::TimeMode(if p.time_mode == "elapsed" {
            TimeMode::Elapsed
        } else {
            TimeMode::Remaining
        }),
        Flx10Setting::VirtualDeckMode(p.virtual_deck),
        Flx10Setting::RecOut(if p.rec_out == "without-mic" {
            RecOut::WithoutMic
        } else {
            RecOut::WithMic
        }),
        Flx10Setting::BoostLevel(boost),
    ]
}

/// The control transfers as text ("index 0x.. value 0x.." per line), shown
/// under the settings so what is written can be checked.
pub(crate) fn flx10_wire_text(p: &Flx10Prefs) -> String {
    flx10_settings(p)
        .into_iter()
        .map(|s| {
            let t = od_controller::flx10_settings::encode(s);
            format!("0x{:04X} ← 0x{:04X}", t.index, t.value)
        })
        .collect::<Vec<_>>()
        .join("   ")
}

/// Writes the settings to the attached DDJ-FLX10.
pub(crate) fn apply_flx10(p: &Flx10Prefs) -> Result<(), String> {
    #[cfg(feature = "flx10-usb")]
    {
        let dev = od_controller::flx10_settings::Flx10Device::open().map_err(|e| e.to_string())?;
        dev.apply(&flx10_settings(p)).map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "flx10-usb"))]
    {
        let _ = p;
        Err(od_i18n::tr("this build has no USB device access (feature flx10-usb)").to_string())
    }
}

/// Applies the stored FLX10 settings and reports the result.
pub(crate) fn apply_flx10_now(app: &App) {
    let p = app.state.borrow().settings.controller.flx10.clone();
    match apply_flx10(&p) {
        Ok(()) => app.status("DDJ-FLX10 settings written"),
        Err(e) => app.status(od_i18n::trf("DDJ-FLX10 settings not written: {}", &[&e])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_controller::monitor::Direction;

    #[test]
    fn monitor_lines_and_learning_from_the_stream() {
        let m = MidiMonitor::new(64);
        let mut t = MidiTools {
            monitor: Some(m.clone()),
            ..Default::default()
        };
        t.set_monitoring(true);
        m.record("DDJ-TEST", Direction::In, &[0x90, 0x0B, 0x7F], Vec::new());
        assert!(t.poll());
        assert_eq!(t.lines.len(), 1);
        assert!(
            t.lines[0].contains("90 0B 7F") || t.lines[0].to_uppercase().contains("90 0B 7F"),
            "{}",
            t.lines[0]
        );
        // Learn a 14-bit fader: CC 0x13 with its LSB 0x33.
        t.start_learn();
        for v in [10u8, 20, 30] {
            m.record("DDJ-TEST", Direction::In, &[0xB0, 0x13, v], Vec::new());
            m.record("DDJ-TEST", Direction::In, &[0xB0, 0x33, 0], Vec::new());
        }
        t.poll();
        let s = t.suggestion.clone().expect("a suggestion");
        assert_eq!((s.addr.number, s.lsb), (0x13, Some(0x33)));
        assert_eq!(t.learn_port, "DDJ-TEST");
        assert!(describe(&s).contains("CC 0x13 / 0x33"));
        t.action = od_controller::templates::ACTION_NAMES
            .iter()
            .position(|a| *a == "mixer.fader")
            .unwrap();
        t.deck = 2;
        let e = t.entry().unwrap();
        assert!(
            e.contains("cc = 0x13")
                && e.contains("lsb = 0x33")
                && e.contains("deck = 2")
                && e.contains("action = \"mixer.fader\""),
            "{e}"
        );
        t.stop_learn();
        t.set_monitoring(false);
        m.record("DDJ-TEST", Direction::In, &[0x90, 0x0C, 0x7F], Vec::new());
        assert!(!t.poll(), "nothing is read while both are off");
    }

    #[test]
    fn learned_entries_make_a_loadable_mapping_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mappings/learned.toml");
        let mut s = LearnSession::new();
        s.feed(&[0x91, 0x0B, 0x7F]);
        s.feed(&[0x91, 0x0B, 0x00]);
        let l = s.suggest().unwrap();
        save_entry(
            &path,
            "DDJ-TEST MIDI 1",
            &input_block(&l, "deck.play_pause", Some(2)),
        )
        .unwrap();
        let m = od_controller::Mapping::load(&path).expect("loads");
        assert!(m.matches_port("DDJ-TEST MIDI 1"));
        let mut s2 = LearnSession::new();
        s2.feed(&[0xB0, 0x40, 0x41]);
        s2.feed(&[0xB0, 0x40, 0x41]);
        s2.feed(&[0xB0, 0x40, 0x3F]);
        save_entry(
            &path,
            "",
            &input_block(&s2.suggest().unwrap(), "browse.rotate", None),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[[input]]").count(), 2);
        assert!(od_controller::Mapping::load(&path).is_ok());
        // An entry the mapping cannot use is refused and the file kept.
        let bad = input_block(&l, "no.such_action", None);
        assert!(save_entry(&path, "", &bad).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn flx10_preferences_encode_like_the_vendor_defaults() {
        let d = Flx10Prefs::default();
        let got = flx10_settings(&d);
        // The defaults are the vendor utility's defaults (plus the boost level).
        assert_eq!(
            &got[..7],
            &od_controller::flx10_settings::vendor_defaults()[..]
        );
        assert_eq!(got[7], Flx10Setting::BoostLevel(BoostLevel::ZeroDb));
        let p = Flx10Prefs {
            fader_start: "off".into(),
            time_mode: "elapsed".into(),
            jog_ring: 3,
            boost_db: 9,
            ..Default::default()
        };
        let s = flx10_settings(&p);
        assert!(
            s.contains(&Flx10Setting::FaderStart(FaderStart::Off))
                && s.contains(&Flx10Setting::TimeMode(TimeMode::Elapsed))
        );
        assert!(
            s.contains(&Flx10Setting::JogRingColor(JogRingColor::Mode3))
                && s.contains(&Flx10Setting::BoostLevel(BoostLevel::Plus9dB))
        );
        assert_eq!(flx10_wire_text(&d).matches('←').count(), 8);
    }
}
