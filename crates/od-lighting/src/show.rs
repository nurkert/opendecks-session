//! The show: turns the music (what the decks play, where they are in their
//! phrases, how loud each is in the mix) or the operator's choices into one
//! [`Look`] per fixture, every frame.
//!
//! Three modes (SPEC §10.5):
//! * **Auto** picks a program from the phrase of the decks you hear: calm
//!   washes in intros, building chases in build-ups, full beat-locked
//!   movement in choruses, breathing in breakdowns, a fade in outros. While
//!   two decks are audible the looks of both are blended by how much of each
//!   reaches the master, so a mix becomes a lighting transition too, and a
//!   phrase change crossfades over one bar.
//! * **Easy** is one program and one colour for the whole rig, beat-locked to
//!   the master deck.
//! * **Pro** sets a program, colour, speed and fixed attributes per group.
//!
//! Master dimmer, blackout, flash and strobe-hold apply on top of every mode.

use std::collections::HashMap;

use od_core::{PhraseKind, PhraseMood};
use serde::{Deserialize, Serialize};

use crate::fixture::{FixtureRole, Patch};
use crate::render::Look;

/// What one deck contributes, filled from the engine state each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeckMusic {
    pub playing: bool,
    pub bpm: f64,
    /// Fractional grid beat at the playhead.
    pub beat: f64,
    pub phrase: Option<PhraseKind>,
    pub mood: Option<PhraseMood>,
    /// Position in the current phrase, 0..1.
    pub phrase_progress: f32,
    /// Track energy, 0..1 (from analysis; 0.5 if unknown).
    pub energy: f32,
    /// Linear gain of the deck to the master (fader × crossfader).
    pub weight: f32,
    /// Base hue of the track, 0..1 (see [`hue_for_key`]).
    pub hue: f32,
}

/// Movement / intensity pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Program {
    /// Steady wash.
    Static,
    /// Flash on every beat, decaying.
    Pulse,
    /// Light running across fixtures / cells.
    Chase,
    /// Movers sweep, others pulse gently.
    Sweep,
    /// Slow intensity breathing over two bars.
    Breathe,
    /// Rising intensity and chase speed through the phrase.
    Build,
    /// Strobe.
    Strobe,
    /// All dark.
    Blackout,
}

impl Program {
    pub const ALL: [Program; 8] = [
        Program::Static,
        Program::Pulse,
        Program::Chase,
        Program::Sweep,
        Program::Breathe,
        Program::Build,
        Program::Strobe,
        Program::Blackout,
    ];

    /// The automatic choice for a phrase.
    pub fn for_phrase(kind: Option<PhraseKind>) -> Program {
        match kind {
            Some(PhraseKind::Intro) => Program::Breathe,
            Some(PhraseKind::Verse) => Program::Chase,
            Some(PhraseKind::Up) => Program::Build,
            Some(PhraseKind::Chorus) => Program::Sweep,
            Some(PhraseKind::Bridge) | Some(PhraseKind::Down) => Program::Breathe,
            Some(PhraseKind::Outro) => Program::Static,
            None => Program::Static,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Auto,
    Easy,
    Pro,
}

/// Per-group settings of Pro mode. `None` follows the automatic choice.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GroupSettings {
    pub program: Option<Program>,
    pub color: Option<[f32; 3]>,
    pub intensity: Option<f32>,
    pub pan: Option<f32>,
    pub tilt: Option<f32>,
    /// Speed multiplier of the program (0.25 … 4, default 1).
    pub speed: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShowSettings {
    pub mode: Mode,
    pub master: f32,
    pub blackout: bool,
    /// Easy mode program and colour (`None` colour = from the track).
    pub easy_program: Program,
    pub easy_color: Option<[f32; 3]>,
    pub groups: HashMap<String, GroupSettings>,
}

impl Default for ShowSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Auto,
            master: 1.0,
            blackout: false,
            easy_program: Program::Pulse,
            easy_color: None,
            groups: HashMap::new(),
        }
    }
}

/// Momentary performance buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Momentary {
    /// Everything full white.
    pub flash: bool,
    /// Everything strobing.
    pub strobe: bool,
}

pub struct Show {
    pub settings: ShowSettings,
    pub momentary: Momentary,
    /// Program of the dominant deck and the frame it changed.
    current: Option<(Program, f32)>,
    /// Previous frame's looks and remaining crossfade (0..1).
    previous: Vec<Look>,
    fade: f32,
    fade_time: f32,
}

impl Default for Show {
    fn default() -> Self {
        Self::new(ShowSettings::default())
    }
}

impl Show {
    pub fn new(settings: ShowSettings) -> Self {
        Self {
            settings,
            momentary: Momentary::default(),
            current: None,
            previous: Vec::new(),
            fade: 0.0,
            fade_time: 1.0,
        }
    }

    /// One frame: `dt` seconds since the last.
    pub fn frame(&mut self, patch: &Patch, decks: &[DeckMusic], dt: f32) -> Vec<Look> {
        let audible: Vec<&DeckMusic> = decks
            .iter()
            .filter(|d| d.playing && d.weight > 0.01)
            .collect();
        let dominant = audible
            .iter()
            .copied()
            .max_by(|a, b| a.weight.total_cmp(&b.weight));

        let mut looks: Vec<Look> = match self.settings.mode {
            Mode::Auto => {
                if audible.is_empty() {
                    idle(patch)
                } else {
                    // Blend each audible deck's look by its share of the mix.
                    let mut acc: Option<Vec<Look>> = None;
                    let mut acc_w = 0.0;
                    for d in &audible {
                        let program = Program::for_phrase(d.phrase);
                        let set = rig(patch, d, |_| (program, None, None, None, None, 1.0));
                        acc_w += d.weight;
                        acc = Some(match acc {
                            None => set,
                            Some(prev) => prev
                                .iter()
                                .zip(&set)
                                .map(|(a, b)| a.blend(b, d.weight / acc_w))
                                .collect(),
                        });
                    }
                    acc.unwrap_or_default()
                }
            }
            Mode::Easy => {
                let music = dominant.copied().unwrap_or(DeckMusic {
                    bpm: 120.0,
                    energy: 0.5,
                    ..Default::default()
                });
                let program = self.settings.easy_program;
                let color = self.settings.easy_color;
                rig(patch, &music, |_| (program, color, None, None, None, 1.0))
            }
            Mode::Pro => {
                let music = dominant.copied().unwrap_or(DeckMusic {
                    bpm: 120.0,
                    energy: 0.5,
                    ..Default::default()
                });
                let groups = &self.settings.groups;
                rig(patch, &music, |group| {
                    let g = groups.get(group).cloned().unwrap_or_default();
                    (
                        g.program
                            .unwrap_or_else(|| Program::for_phrase(music.phrase)),
                        g.color,
                        g.intensity,
                        g.pan,
                        g.tilt,
                        g.speed.unwrap_or(1.0),
                    )
                })
            }
        };

        // Phrase change of the dominant deck: crossfade over one bar.
        let program = dominant.map(|d| Program::for_phrase(d.phrase));
        let bar = dominant
            .map(|d| (240.0 / d.bpm.max(1.0)) as f32)
            .unwrap_or(2.0);
        match (self.current, program) {
            (Some((old, _)), Some(new)) if old != new && self.settings.mode == Mode::Auto => {
                self.fade = 1.0;
                self.fade_time = bar;
                self.current = Some((new, 0.0));
            }
            (_, Some(new)) => self.current = Some((new, 0.0)),
            _ => {}
        }
        if self.fade > 0.0 && self.previous.len() == looks.len() {
            let t = 1.0 - self.fade;
            looks = self
                .previous
                .iter()
                .zip(&looks)
                .map(|(a, b)| a.blend(b, t))
                .collect();
            self.fade = (self.fade - dt / self.fade_time.max(0.05)).max(0.0);
        }
        self.previous = looks.clone();

        // Master layer.
        let master = if self.settings.blackout {
            0.0
        } else {
            self.settings.master.clamp(0.0, 1.0)
        };
        for look in &mut looks {
            if self.momentary.flash {
                look.intensity = 1.0;
                look.color = [1.0; 3];
                look.pixels.clear();
            }
            if self.momentary.strobe {
                look.intensity = 1.0;
                look.strobe = 1.0;
            }
            look.intensity *= master;
            for p in &mut look.pixels {
                *p = p.map(|c| c * master);
            }
        }
        looks
    }
}

type GroupChoice = (
    Program,
    Option<[f32; 3]>,
    Option<f32>,
    Option<f32>,
    Option<f32>,
    f32,
);

fn idle(patch: &Patch) -> Vec<Look> {
    patch
        .fixtures
        .iter()
        .map(|f| Look {
            intensity: if f.def.role() == FixtureRole::Wash {
                0.15
            } else {
                0.0
            },
            color: [0.35, 0.45, 1.0],
            ..Look::default()
        })
        .collect()
}

/// Looks for the whole rig from one deck's music.
fn rig(patch: &Patch, music: &DeckMusic, choose: impl Fn(&str) -> GroupChoice) -> Vec<Look> {
    // Index of each fixture among its role, for chases and fans.
    let mut counts: HashMap<FixtureRole, usize> = HashMap::new();
    let roles: Vec<(FixtureRole, usize)> = patch
        .fixtures
        .iter()
        .map(|f| {
            let role = f.def.role();
            let n = counts.entry(role).or_default();
            *n += 1;
            (role, *n - 1)
        })
        .collect();
    patch
        .fixtures
        .iter()
        .zip(roles)
        .map(|(f, (role, index))| {
            let (program, color, intensity, pan, tilt, speed) = choose(&f.group);
            let ctx = Ctx {
                role,
                index,
                count: counts[&role],
                cells: f.def.pixels() as usize,
                beat: music.beat * speed as f64,
                progress: music.phrase_progress,
                energy: if music.energy > 0.0 {
                    music.energy
                } else {
                    0.5
                },
                color: color.unwrap_or_else(|| palette(music.hue, music.mood, music.beat)),
            };
            let mut look = program_look(program, &ctx);
            if let Some(i) = intensity {
                look.intensity = i;
            }
            if let Some(p) = pan {
                look.pan = p;
            }
            if let Some(t) = tilt {
                look.tilt = t;
            }
            look
        })
        .collect()
}

struct Ctx {
    role: FixtureRole,
    index: usize,
    count: usize,
    cells: usize,
    beat: f64,
    progress: f32,
    energy: f32,
    color: [f32; 3],
}

fn frac(x: f64) -> f32 {
    (x - x.floor()) as f32
}

/// Beat flash shape: 1 on the beat, decaying over it.
fn pulse(beat: f64) -> f32 {
    let p = frac(beat);
    (1.0 - p).powf(2.5)
}

fn program_look(program: Program, c: &Ctx) -> Look {
    let tau = std::f32::consts::TAU;
    let phase = c.index as f32 / c.count.max(1) as f32;
    let mut look = Look {
        color: c.color,
        ..Look::default()
    };
    // Movers: home position with a slight fan.
    look.pan = 0.5 + (phase - 0.5) * 0.3;
    look.tilt = 0.6;
    let base = 0.35 + 0.65 * c.energy;
    match program {
        Program::Blackout => look.intensity = 0.0,
        Program::Static => look.intensity = base * 0.8,
        Program::Breathe => {
            let b = (frac(c.beat / 8.0) * tau).sin() * 0.5 + 0.5;
            look.intensity = 0.2 + 0.4 * b * base;
            look.pan = 0.5 + 0.15 * (frac(c.beat / 16.0) * tau + phase * tau).sin();
        }
        Program::Pulse => look.intensity = 0.25 + 0.75 * pulse(c.beat) * base,
        Program::Chase => {
            let step = c.beat.floor() as usize;
            let on = c.count <= 1 || step % c.count == c.index;
            look.intensity = if on { base } else { 0.15 * base };
            look.pixels = chase_cells(c.cells, c.beat, c.color);
            look.pan = 0.5 + 0.25 * (frac(c.beat / 8.0) * tau + phase * tau).sin();
            look.tilt = 0.55 + 0.15 * (frac(c.beat / 8.0) * tau * 2.0).sin();
        }
        Program::Sweep => {
            look.intensity = (0.6 + 0.4 * pulse(c.beat)) * base.max(0.8);
            // Fan out and sweep over four beats, alternate sides.
            let dir = if c.index.is_multiple_of(2) { 1.0 } else { -1.0 };
            look.pan = 0.5 + dir * 0.35 * (frac(c.beat / 4.0) * tau).sin();
            look.tilt = 0.5 + 0.25 * (frac(c.beat / 2.0) * tau).cos();
            look.pixels = chase_cells(c.cells, c.beat * 2.0, c.color);
        }
        Program::Build => {
            let p = c.progress.clamp(0.0, 1.0);
            // Chase speeds up through the phrase: 1, 2, then 4 steps per beat.
            let rate = if p < 0.5 {
                1.0
            } else if p < 0.875 {
                2.0
            } else {
                4.0
            };
            look.intensity = (0.3 + 0.7 * p) * (0.5 + 0.5 * pulse(c.beat * rate));
            look.pixels = chase_cells(c.cells, c.beat * rate, c.color);
            look.tilt = 0.6 - 0.3 * p; // rise towards the audience
            if p >= 0.9375 {
                look.strobe = 0.6 + 0.4 * (p - 0.9375) * 16.0;
                look.intensity = 1.0;
            }
        }
        Program::Strobe => {
            look.intensity = 1.0;
            look.strobe = 0.8;
            look.color = [1.0; 3];
        }
    }
    if c.role == FixtureRole::Effect && program != Program::Strobe && program != Program::Build {
        // Plain strobes stay dark unless strobing.
        look.intensity = 0.0;
    }
    look
}

/// A single lit cell running along a bar with a soft tail.
fn chase_cells(cells: usize, beat: f64, color: [f32; 3]) -> Vec<[f32; 3]> {
    if cells <= 1 {
        return Vec::new();
    }
    let head = frac(beat) * cells as f32;
    (0..cells)
        .map(|i| {
            let d = (head - i as f32).rem_euclid(cells as f32);
            let v = (1.0 - d / 3.0).max(0.0);
            color.map(|c| c * v)
        })
        .collect()
}

/// Colour of the moment: the track hue, shifted with the phrase mood, and a
/// complementary accent every other bar in high-energy moods.
fn palette(hue: f32, mood: Option<PhraseMood>, beat: f64) -> [f32; 3] {
    let accent = matches!(mood, Some(PhraseMood::High)) && (beat / 4.0).floor() as i64 % 2 == 1;
    let h = if accent { hue + 0.5 } else { hue };
    let s = match mood {
        Some(PhraseMood::Low) => 0.9,
        _ => 0.8,
    };
    hsv(h, s, 1.0)
}

/// Hue of a Camelot key number (1..=12) so harmonic neighbours look alike.
pub fn hue_for_key(camelot: u8) -> f32 {
    ((camelot.max(1) - 1) % 12) as f32 / 12.0
}

pub fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as u8 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{FixtureDef, Patched};

    fn rig_patch() -> Patch {
        let mut patch = Patch::default();
        for (i, kind) in [
            "rgbw-par",
            "rgbw-par",
            "moving-head",
            "moving-head",
            "light-bar-8",
        ]
        .iter()
        .enumerate()
        {
            let def = FixtureDef::builtin(kind).unwrap();
            let address = patch.next_free(0, def.channels.len() as u16).unwrap();
            let group = if kind.contains("par") {
                "Wash"
            } else if kind.contains("head") {
                "Movers"
            } else {
                "Bars"
            };
            patch
                .add(Patched {
                    name: format!("{kind} {i}"),
                    def,
                    universe: 0,
                    address,
                    group: group.into(),
                    invert_pan: false,
                })
                .unwrap();
        }
        patch
    }

    fn deck(phrase: PhraseKind, beat: f64, progress: f32) -> DeckMusic {
        DeckMusic {
            playing: true,
            bpm: 128.0,
            beat,
            phrase: Some(phrase),
            phrase_progress: progress,
            energy: 0.8,
            weight: 1.0,
            ..Default::default()
        }
    }

    #[test]
    fn auto_follows_the_phrase() {
        let patch = rig_patch();
        let mut show = Show::default();
        let intro = show.frame(&patch, &[deck(PhraseKind::Intro, 0.5, 0.1)], 0.02);
        let mut show = Show::default();
        let chorus = show.frame(&patch, &[deck(PhraseKind::Chorus, 0.0, 0.1)], 0.02);
        assert!(
            chorus[0].intensity > intro[0].intensity,
            "chorus is brighter than intro"
        );
        // Movers move during a chorus.
        let later = show.frame(&patch, &[deck(PhraseKind::Chorus, 1.0, 0.1)], 0.02);
        assert_ne!(chorus[2].pan, later[2].pan);
    }

    #[test]
    fn build_up_ends_in_a_strobe() {
        let patch = rig_patch();
        let mut show = Show::default();
        let early = show.frame(&patch, &[deck(PhraseKind::Up, 0.0, 0.1)], 0.02);
        let end = show.frame(&patch, &[deck(PhraseKind::Up, 0.0, 0.97)], 0.02);
        assert_eq!(early[0].strobe, 0.0);
        assert!(end[0].strobe > 0.5);
    }

    #[test]
    fn a_mix_blends_both_decks() {
        let patch = rig_patch();
        let a = DeckMusic {
            hue: 0.0,
            ..deck(PhraseKind::Outro, 0.0, 0.5)
        };
        let b = DeckMusic {
            hue: 2.0 / 3.0,
            ..deck(PhraseKind::Outro, 0.0, 0.5)
        };
        let only_a = Show::default().frame(&patch, &[a], 0.02);
        let half = Show::default().frame(
            &patch,
            &[
                DeckMusic { weight: 0.5, ..a },
                DeckMusic { weight: 0.5, ..b },
            ],
            0.02,
        );
        assert!(only_a[0].color[2] < 0.3, "red-ish track");
        assert!(half[0].color[2] > only_a[0].color[2], "blue track mixed in");
        assert!(half[0].color[0] < only_a[0].color[0]);
    }

    #[test]
    fn phrase_change_crossfades_over_a_bar() {
        let patch = rig_patch();
        let mut show = Show::default();
        let intro = show.frame(&patch, &[deck(PhraseKind::Outro, 0.0, 0.5)], 0.02);
        let first = show.frame(&patch, &[deck(PhraseKind::Chorus, 0.0, 0.0)], 0.02);
        let chorus = Show::default().frame(&patch, &[deck(PhraseKind::Chorus, 0.0, 0.0)], 0.02);
        // Right after the change the rig is still close to the old look…
        assert!(
            (first[0].intensity - intro[0].intensity).abs()
                < (chorus[0].intensity - intro[0].intensity).abs()
        );
        // …and after a bar (1.875 s at 128 BPM) it has arrived.
        let mut last = first;
        for _ in 0..100 {
            last = show.frame(&patch, &[deck(PhraseKind::Chorus, 0.0, 0.0)], 0.02);
        }
        assert!((last[0].intensity - chorus[0].intensity).abs() < 1e-4);
    }

    #[test]
    fn manual_modes_and_master() {
        let patch = rig_patch();
        let mut show = Show::new(ShowSettings {
            mode: Mode::Easy,
            easy_program: Program::Static,
            easy_color: Some([0.0, 1.0, 0.0]),
            ..Default::default()
        });
        let looks = show.frame(&patch, &[], 0.02);
        assert_eq!(looks[0].color, [0.0, 1.0, 0.0]);
        assert!(looks[0].intensity > 0.0);

        show.settings.mode = Mode::Pro;
        show.settings.groups.insert(
            "Movers".into(),
            GroupSettings {
                pan: Some(0.1),
                tilt: Some(0.9),
                program: Some(Program::Static),
                ..Default::default()
            },
        );
        let looks = show.frame(&patch, &[], 0.02);
        assert_eq!((looks[2].pan, looks[2].tilt), (0.1, 0.9));

        show.settings.blackout = true;
        assert!(
            show.frame(&patch, &[], 0.02)
                .iter()
                .all(|l| l.intensity == 0.0)
        );
        show.settings.blackout = false;
        show.momentary.strobe = true;
        assert!(show.frame(&patch, &[], 0.02).iter().all(|l| l.strobe > 0.0));
    }

    #[test]
    fn idle_without_music_keeps_a_soft_wash() {
        let patch = rig_patch();
        let looks = Show::default().frame(&patch, &[], 0.02);
        assert!(looks[0].intensity > 0.0 && looks[0].intensity < 0.3);
        assert_eq!(looks[2].intensity, 0.0);
    }
}
