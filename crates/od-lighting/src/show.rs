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
    /// Library id of the loaded track (for per-track lighting cues).
    pub track: Option<i64>,
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
    /// Attribute effect layered over the program.
    #[serde(default)]
    pub effect: Option<Effect>,
}

/// Attribute an [`Effect`] moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attr {
    Dimmer,
    Pan,
    Tilt,
}

/// Wave shape of an [`Effect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wave {
    Sine,
    Saw,
    Square,
    Random,
}

/// A beat-synced attribute effect (Pro mode): a wave over `beats` beats,
/// `depth` 0..1, with the phase spread across the group's fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub attr: Attr,
    pub wave: Wave,
    pub beats: f32,
    pub depth: f32,
    /// Phase offset between neighbouring fixtures, 0..1 of a cycle.
    pub spread: f32,
}

impl Effect {
    /// Wave value 0..1 at `beat` for fixture `index`.
    pub fn value(&self, beat: f64, index: usize) -> f32 {
        let cycle = beat / self.beats.max(0.0625) as f64 + index as f64 * self.spread as f64;
        let p = (cycle - cycle.floor()) as f32;
        match self.wave {
            Wave::Sine => 0.5 - 0.5 * (p * std::f32::consts::TAU).cos(),
            Wave::Saw => p,
            Wave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Wave::Random => {
                // A new value per cycle, stable within it.
                let n = cycle.floor() as i64 as u64
                    ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                let x = n.wrapping_mul(0x2545_F491_4F6C_DD1D);
                ((x >> 40) & 0xFFFF) as f32 / 65535.0
            }
        }
    }

    /// Applies the effect to a look.
    pub fn apply(&self, look: &mut Look, beat: f64, index: usize) {
        let v = self.value(beat, index);
        let d = self.depth.clamp(0.0, 1.0);
        match self.attr {
            Attr::Dimmer => look.intensity *= 1.0 - d + d * v,
            Attr::Pan => look.pan = (look.pan + (v - 0.5) * d).clamp(0.0, 1.0),
            Attr::Tilt => look.tilt = (look.tilt + (v - 0.5) * d).clamp(0.0, 1.0),
        }
    }
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
    /// Pro mode cue stack.
    #[serde(default)]
    pub cues: Vec<LightCue>,
    /// Lighting cues stored with tracks: library track id → cues by beat.
    #[serde(default)]
    pub track_cues: HashMap<i64, Vec<TrackCue>>,
}

/// One cue of the Pro mode stack: group settings layered over the base
/// groups, reached with a fade.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LightCue {
    pub name: String,
    pub groups: HashMap<String, GroupSettings>,
    /// Fade into this cue, in beats of the playing deck.
    pub fade_beats: f32,
}

/// A cue fired when the track reaches a beat.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackCue {
    pub beat: u32,
    /// Index into [`ShowSettings::cues`].
    pub cue: usize,
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
            cues: Vec::new(),
            track_cues: HashMap::new(),
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
    /// Fog machines on.
    pub fog: bool,
}

pub struct Show {
    pub settings: ShowSettings,
    pub momentary: Momentary,
    /// Cue chosen by Go / Back (track cues take precedence while they apply).
    pub manual_cue: Option<usize>,
    /// Cue in effect last frame.
    active_cue: Option<usize>,
    /// Program of the dominant deck and the frame it changed.
    current: Option<(Program, f32)>,
    /// Previous frame's looks and remaining crossfade (0..1).
    previous: Vec<Look>,
    fade: f32,
    fade_time: f32,
    /// Tapped tempo for Easy / Pro when no deck plays.
    pub tap_bpm: Option<f64>,
    /// Free-running beat when no deck plays.
    free_beat: f64,
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
            manual_cue: None,
            active_cue: None,
            current: None,
            previous: Vec::new(),
            fade: 0.0,
            fade_time: 1.0,
            tap_bpm: None,
            free_beat: 0.0,
        }
    }

    /// Music to follow when no deck plays: the tapped (or 120) BPM.
    fn free_music(&mut self, dt: f32) -> DeckMusic {
        let bpm = self.tap_bpm.unwrap_or(120.0);
        self.free_beat += dt as f64 * bpm / 60.0;
        DeckMusic {
            bpm,
            beat: self.free_beat,
            energy: 0.6,
            ..Default::default()
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
                        let set = rig(patch, d, |_| Choice {
                            program,
                            ..Choice::default()
                        });
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
                let music = match dominant {
                    Some(d) => *d,
                    None => self.free_music(dt),
                };
                let program = self.settings.easy_program;
                let color = self.settings.easy_color;
                rig(patch, &music, |_| Choice {
                    program,
                    color,
                    ..Choice::default()
                })
            }
            Mode::Pro => {
                let music = match dominant {
                    Some(d) => *d,
                    None => self.free_music(dt),
                };
                let groups = &self.settings.groups;
                let cue = self
                    .cue_for(dominant.copied())
                    .and_then(|i| self.settings.cues.get(i));
                rig(patch, &music, |group| {
                    let base = groups.get(group).cloned().unwrap_or_default();
                    let g = match cue.and_then(|c| c.groups.get(group)) {
                        Some(o) => GroupSettings {
                            program: o.program.or(base.program),
                            color: o.color.or(base.color),
                            intensity: o.intensity.or(base.intensity),
                            pan: o.pan.or(base.pan),
                            tilt: o.tilt.or(base.tilt),
                            speed: o.speed.or(base.speed),
                            effect: o.effect.or(base.effect),
                        },
                        None => base,
                    };
                    Choice {
                        program: g
                            .program
                            .unwrap_or_else(|| Program::for_phrase(music.phrase)),
                        color: g.color,
                        intensity: g.intensity,
                        pan: g.pan,
                        tilt: g.tilt,
                        speed: g.speed.unwrap_or(1.0),
                        effect: g.effect,
                    }
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
        // Pro cue change: fade over the cue's beats.
        if self.settings.mode == Mode::Pro {
            let cue = self.cue_for(dominant.copied());
            if cue != self.active_cue {
                let beats = cue
                    .and_then(|i| self.settings.cues.get(i))
                    .map_or(0.0, |c| c.fade_beats);
                let bpm = dominant.map_or(120.0, |d| d.bpm.max(1.0)) as f32;
                if beats > 0.0 {
                    self.fade = 1.0;
                    self.fade_time = beats * 60.0 / bpm;
                }
                self.active_cue = cue;
            }
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
            look.fog = self.momentary.fog;
            look.intensity *= master;
            for p in &mut look.pixels {
                *p = p.map(|c| c * master);
            }
        }
        looks
    }
}

impl Show {
    /// Cue in effect: the last track cue the dominant deck has passed, else
    /// the manually chosen one.
    fn cue_for(&self, deck: Option<DeckMusic>) -> Option<usize> {
        let from_track = deck.and_then(|d| {
            let cues = self.settings.track_cues.get(&d.track?)?;
            cues.iter()
                .filter(|c| (c.beat as f64) <= d.beat)
                .max_by_key(|c| c.beat)
                .map(|c| c.cue)
        });
        from_track
            .or(self.manual_cue)
            .filter(|&i| i < self.settings.cues.len())
    }

    /// Next cue in the stack.
    pub fn go(&mut self) {
        let n = self.settings.cues.len();
        if n > 0 {
            self.manual_cue = Some(self.manual_cue.map_or(0, |i| (i + 1) % n));
        }
    }

    /// Previous cue.
    pub fn back(&mut self) {
        let n = self.settings.cues.len();
        if n > 0 {
            self.manual_cue = Some(self.manual_cue.map_or(n - 1, |i| (i + n - 1) % n));
        }
    }

    /// Cue in effect last frame (for the UI).
    pub fn active_cue(&self) -> Option<usize> {
        self.active_cue
    }
}

/// What a group does this frame.
struct Choice {
    program: Program,
    color: Option<[f32; 3]>,
    intensity: Option<f32>,
    pan: Option<f32>,
    tilt: Option<f32>,
    speed: f32,
    effect: Option<Effect>,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            program: Program::Static,
            color: None,
            intensity: None,
            pan: None,
            tilt: None,
            speed: 1.0,
            effect: None,
        }
    }
}

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
fn rig(patch: &Patch, music: &DeckMusic, choose: impl Fn(&str) -> Choice) -> Vec<Look> {
    // Index of each fixture within its group, for effect phase spread.
    let mut in_group: HashMap<&str, usize> = HashMap::new();
    let group_index: Vec<usize> = patch
        .fixtures
        .iter()
        .map(|f| {
            let n = in_group.entry(f.group.as_str()).or_default();
            *n += 1;
            *n - 1
        })
        .collect();
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
        .zip(group_index)
        .map(|((f, (role, index)), gi)| {
            let Choice {
                program,
                color,
                intensity,
                pan,
                tilt,
                speed,
                effect,
            } = choose(&f.group);
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
            if let Some(e) = effect {
                e.apply(&mut look, music.beat * speed as f64, gi);
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
    fn pro_cue_stack_and_track_cues() {
        let patch = rig_patch();
        let red = |name: &str, color: [f32; 3]| LightCue {
            name: name.into(),
            groups: HashMap::from([(
                "Wash".to_string(),
                GroupSettings {
                    program: Some(Program::Static),
                    color: Some(color),
                    ..Default::default()
                },
            )]),
            fade_beats: 0.0,
        };
        let mut settings = ShowSettings {
            mode: Mode::Pro,
            ..Default::default()
        };
        settings.cues = vec![red("Red", [1.0, 0.0, 0.0]), red("Blue", [0.0, 0.0, 1.0])];
        let mut show = Show::new(settings);
        show.go();
        assert_eq!(show.frame(&patch, &[], 0.02)[0].color, [1.0, 0.0, 0.0]);
        show.go();
        assert_eq!(show.frame(&patch, &[], 0.02)[0].color, [0.0, 0.0, 1.0]);
        assert_eq!(show.active_cue(), Some(1));
        show.back();
        assert_eq!(show.frame(&patch, &[], 0.02)[0].color, [1.0, 0.0, 0.0]);

        // A track cue at beat 32 takes over once the deck passes it.
        show.settings
            .track_cues
            .insert(7, vec![TrackCue { beat: 32, cue: 1 }]);
        let at = |beat| DeckMusic {
            track: Some(7),
            ..deck(PhraseKind::Verse, beat, 0.5)
        };
        assert_eq!(
            show.frame(&patch, &[at(16.0)], 0.02)[0].color,
            [1.0, 0.0, 0.0]
        );
        assert_eq!(
            show.frame(&patch, &[at(33.0)], 0.02)[0].color,
            [0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn idle_without_music_keeps_a_soft_wash() {
        let patch = rig_patch();
        let looks = Show::default().frame(&patch, &[], 0.02);
        assert!(looks[0].intensity > 0.0 && looks[0].intensity < 0.3);
        assert_eq!(looks[2].intensity, 0.0);
    }

    #[test]
    fn effects_spread_across_a_group_on_the_beat() {
        let e = Effect {
            attr: Attr::Dimmer,
            wave: Wave::Sine,
            beats: 4.0,
            depth: 1.0,
            spread: 0.5,
        };
        // Opposite phases for neighbours, a full cycle every four beats.
        assert!((e.value(0.0, 0) - 0.0).abs() < 1e-6);
        assert!((e.value(0.0, 1) - 1.0).abs() < 1e-6);
        assert!((e.value(2.0, 0) - 1.0).abs() < 1e-6);
        let sq = Effect {
            wave: Wave::Square,
            ..e
        };
        assert_eq!((sq.value(0.5, 0), sq.value(2.5, 0)), (1.0, 0.0));
        let rnd = Effect {
            wave: Wave::Random,
            ..e
        };
        assert_eq!(
            rnd.value(0.1, 0),
            rnd.value(3.9, 0),
            "stable within a cycle"
        );
        // In Pro mode the movers' pan follows the effect.
        let patch = rig_patch();
        let mut show = Show::new(ShowSettings {
            mode: Mode::Pro,
            ..Default::default()
        });
        show.settings.groups.insert(
            "Movers".into(),
            GroupSettings {
                program: Some(Program::Static),
                effect: Some(Effect {
                    attr: Attr::Pan,
                    wave: Wave::Saw,
                    beats: 1.0,
                    depth: 0.8,
                    spread: 0.25,
                }),
                ..Default::default()
            },
        );
        let a = show.frame(&patch, &[deck(PhraseKind::Verse, 0.0, 0.5)], 0.02);
        let b = show.frame(&patch, &[deck(PhraseKind::Verse, 0.5, 0.5)], 0.02);
        assert_ne!(a[2].pan, b[2].pan, "pan moves with the beat");
        assert_ne!(a[2].pan, a[3].pan, "the second mover is out of phase");
    }

    #[test]
    fn fog_and_tap_tempo_without_music() {
        let mut patch = rig_patch();
        let def = FixtureDef::builtin("fog").unwrap();
        let address = patch.next_free(0, 1).unwrap();
        patch
            .add(Patched {
                name: "Fog".into(),
                def,
                universe: 0,
                address,
                group: "FX".into(),
                invert_pan: false,
            })
            .unwrap();
        let mut show = Show::new(ShowSettings {
            mode: Mode::Easy,
            easy_program: Program::Pulse,
            ..Default::default()
        });
        show.momentary.fog = true;
        let looks = show.frame(&patch, &[], 0.02);
        let u = crate::render(&patch, &looks);
        assert_eq!(u[0][address as usize - 1], 255, "fog output on");
        // With no deck playing the easy look runs at the tapped tempo.
        show.momentary.fog = false;
        show.tap_bpm = Some(60.0);
        let first = show.frame(&patch, &[], 0.0)[0].intensity;
        let half = show.frame(&patch, &[], 0.5)[0].intensity;
        assert!(
            half < first,
            "pulse decays within the beat: {first} → {half}"
        );
    }
}
