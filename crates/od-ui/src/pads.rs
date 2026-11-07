//! Deck pad modes (SPEC §10.1): Hot Cue, Pad FX, Beat Jump, Sampler,
//! Keyboard, Key Shift, Beat Loop, Slicer, Stems and Memory Cue.
//!
//! The eight pads of a deck show and do what the deck's pad mode says. The
//! functions here are pure: they project the engine state into pad rows and
//! turn a pad press into engine commands; `decks` and `controller` call them
//! for the screen pads and the controller's pads alike.

use crate::PadView;
use crate::sampler::{SlotInfo, slot_of};
use od_controller::PadMode;
use od_engine::{
    ChannelState, Command, DeckCommand, DeckState, HOT_CUE_BANKS, HOT_CUES, PadFxCommand,
    PadFxEffect, SamplerCommand, SamplerSlotState, SlicerCommand,
};

/// Pad modes offered on screen, in selector order (Pad FX page 2 is
/// reached with the ‹ › of the Pad FX mode).
pub(crate) const UI_MODES: [PadMode; 13] = [
    PadMode::HotCue,
    PadMode::PadFx1,
    PadMode::BeatJump,
    PadMode::Sampler,
    PadMode::Keyboard,
    PadMode::KeyShift,
    PadMode::BeatLoop,
    PadMode::Slicer,
    PadMode::Stems,
    PadMode::MemoryCue,
    PadMode::Lighting,
    PadMode::ActiveCensor,
    PadMode::SeqCall,
];

/// Looks of the Lighting pads A–F (G strobe, H flash).
pub(crate) const LIGHT_LOOKS: [od_lighting::Program; 6] = [
    od_lighting::Program::Static,
    od_lighting::Program::Pulse,
    od_lighting::Program::Chase,
    od_lighting::Program::Sweep,
    od_lighting::Program::Breathe,
    od_lighting::Program::Build,
];

/// Parts of the Stems pads: A–D (mute) and E–H (solo) in this order.
pub(crate) const STEM_PADS: [od_engine::Stem; 4] = [
    od_engine::Stem::Vocals,
    od_engine::Stem::Inst,
    od_engine::Stem::Drums,
    od_engine::Stem::Bass,
];

/// Semitones of the Keyboard and Key Shift pads: A–D = −3 … 0 (left
/// column), E–H = +1 … +4 (right column).
pub(crate) const KEY_STEPS: [f32; 8] = [-3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0];

/// Slicer domain lengths (beats).
pub(crate) const SLICER_BEATS: [f64; 5] = [4.0, 8.0, 16.0, 32.0, 64.0];

/// Beat jump sizes of the ‹ › in Beat Jump mode and the deck's −/+ buttons.
pub(crate) const JUMP_SIZES: [f64; 10] = [0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0];

/// Beat lengths of the Beat Loop pads.
pub(crate) fn pad_beats(index: usize) -> f64 {
    [0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0][index.min(7)]
}

/// Beat jump of a pad: left column (A–D) back, right column (E–H) forward.
pub(crate) fn pad_jump(index: usize) -> f64 {
    let b = [1.0, 2.0, 4.0, 8.0][index % 4];
    if index < 4 { -b } else { b }
}

/// What a deck's pads need besides the engine state.
pub(crate) struct PadCtx<'a> {
    /// 0-based deck.
    pub deck: usize,
    pub mode: PadMode,
    /// Sampler bank shown (global).
    pub sampler_bank: usize,
    /// Hot cue the Keyboard pads play (0..8 in the active bank).
    pub keyboard_cue: usize,
    /// Slicer domain in beats.
    pub slicer_beats: f64,
    /// Hot cue labels by library index (bank * 8 + slot).
    pub labels: &'a [String],
    /// Sampler slot contents (names).
    pub sampler_slots: &'a [Option<SlotInfo>],
    /// Memory cue starts (seconds, track order).
    pub memory: &'a [f64],
}

/// Hot cue letter of library index `i`: A … Z, then AA, AB, …
pub(crate) fn hot_letter(i: usize) -> String {
    if i < 26 {
        ((b'A' + i as u8) as char).to_string()
    } else {
        format!("A{}", (b'A' + (i - 26) as u8) as char)
    }
}

/// "A–H", "I–P", "Q–X", "Y–AF".
pub(crate) fn bank_label(bank: usize) -> String {
    format!(
        "{}–{}",
        hot_letter(bank * HOT_CUES),
        hot_letter(bank * HOT_CUES + HOT_CUES - 1)
    )
}

/// The screen mode `delta` places from `mode` (Pad FX page 2 counts as Pad
/// FX), wrapping.
pub(crate) fn step_mode(mode: PadMode, delta: i32) -> PadMode {
    let m = if mode == PadMode::PadFx2 {
        PadMode::PadFx1
    } else {
        mode
    };
    let i = UI_MODES.iter().position(|x| *x == m).unwrap_or(0) as i32;
    UI_MODES[(i + delta).rem_euclid(UI_MODES.len() as i32) as usize]
}

/// Sampler slot of a deck pad: odd decks play pads 1–8 of the bank, even
/// decks 9–16 (as the two halves of the hardware sampler).
pub(crate) fn sampler_slot(deck: usize, bank: usize, pad: usize) -> usize {
    slot_of(bank, (deck % 2) * 8 + pad.min(7))
}

/// Key shift readout: "±0", "+2", "−3".
pub(crate) fn key_shift_label(semitones: f32) -> String {
    let r = semitones.round() as i32;
    match r {
        0 => "±0".into(),
        r if r > 0 => format!("+{r}"),
        r => format!("−{}", -r),
    }
}

/// A beat count as "1/8", "4", "64".
pub(crate) fn beats_label(b: f64) -> String {
    if b < 1.0 {
        format!("1/{}", (1.0 / b).round())
    } else {
        format!("{}", b.round())
    }
}

/// The value `delta` steps from `current` in `list` (nearest first), clamped.
pub(crate) fn step_in(list: &[f64], current: f64, delta: i32) -> f64 {
    let i = list
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    list[(i as i32 + delta).clamp(0, list.len() as i32 - 1) as usize]
}

/// The ‹ › value beside the mode selector ("" = none for this mode).
pub(crate) fn param_label(s: &DeckState, ctx: &PadCtx) -> String {
    match ctx.mode {
        PadMode::HotCue => bank_label(s.hot_cue_bank),
        PadMode::PadFx1 => "PAGE 1".into(),
        PadMode::PadFx2 => "PAGE 2".into(),
        PadMode::BeatJump => format!("JUMP {}", beats_label(s.beat_jump_size)),
        PadMode::Sampler => format!("BANK {}", ctx.sampler_bank + 1),
        PadMode::Keyboard => format!(
            "CUE {}",
            hot_letter(s.hot_cue_bank * HOT_CUES + ctx.keyboard_cue)
        ),
        PadMode::KeyShift => format!("KEY {}", key_shift_label(s.key_shift)),
        PadMode::BeatLoop => String::new(),
        PadMode::Slicer => format!("{} BEATS", beats_label(ctx.slicer_beats)),
        PadMode::Stems => {
            if s.stems_available {
                "MUTE · SOLO".into()
            } else {
                "NO PARTS".into()
            }
        }
        PadMode::MemoryCue => format!("{} CUES", ctx.memory.len()),
        PadMode::Lighting => "LIGHTS".into(),
        PadMode::ActiveCensor => "CENSOR".into(),
        PadMode::SeqCall => "SEQUENCES".into(),
    }
}

fn pad(
    index: usize,
    caption: impl Into<String>,
    sub: impl Into<String>,
    set: bool,
    lit: bool,
    cue_slot: i32,
) -> PadView {
    PadView {
        index: index as i32,
        caption: caption.into().into(),
        sub: sub.into().into(),
        set,
        lit,
        cue_slot,
        tone: Default::default(),
    }
}

/// The eight pads of a deck in its mode (pure).
pub(crate) fn pad_views(
    s: &DeckState,
    ch: &ChannelState,
    sampler: &[SamplerSlotState],
    ctx: &PadCtx,
) -> Vec<PadView> {
    (0..8)
        .map(|i| match ctx.mode {
            PadMode::HotCue => {
                let idx = s.hot_cue_bank * HOT_CUES + i;
                let hc = s.hot_cues[i];
                let label = ctx.labels.get(idx).cloned().unwrap_or_default();
                let sub = if !label.is_empty() {
                    label
                } else {
                    hc.map(|h| crate::format::deck_time(h.start))
                        .unwrap_or_default()
                };
                pad(i, hot_letter(idx), sub, hc.is_some(), false, i as i32)
            }
            PadMode::PadFx1 | PadMode::PadFx2 => {
                let page = usize::from(ctx.mode == PadMode::PadFx2);
                let p = crate::fx::pad_fx_preset(page, i);
                let lit = ch.pad_fx.held && ch.pad_fx.preset == Some(p);
                let kind = if matches!(p.effect, PadFxEffect::Release(_)) {
                    "RELEASE"
                } else {
                    ""
                };
                pad(i, crate::fx::pad_fx_caption(&p), kind, true, lit, -1)
            }
            PadMode::BeatJump => {
                let b = pad_jump(i);
                pad(
                    i,
                    if b < 0.0 {
                        format!("−{}", -b)
                    } else {
                        format!("+{b}")
                    },
                    if b < 0.0 { "BACK" } else { "FWD" },
                    true,
                    false,
                    -1,
                )
            }
            PadMode::Sampler => {
                let slot = sampler_slot(ctx.deck, ctx.sampler_bank, i);
                let st = sampler.get(slot).copied().unwrap_or_default();
                let name = ctx
                    .sampler_slots
                    .get(slot)
                    .and_then(|x| x.as_ref())
                    .map(|x| x.name.clone())
                    .unwrap_or_default();
                pad(
                    i,
                    format!("{}", slot % 16 + 1),
                    name,
                    st.loaded,
                    st.playing,
                    -1,
                )
            }
            PadMode::Keyboard => {
                let semis = KEY_STEPS[i];
                let base = s.hot_cues[ctx.keyboard_cue.min(HOT_CUES - 1)].is_some();
                let lit = base && (s.key_shift - semis).abs() < 0.01 && !s.key_sync;
                pad(
                    i,
                    key_shift_label(semis),
                    hot_letter(s.hot_cue_bank * HOT_CUES + ctx.keyboard_cue),
                    base,
                    lit,
                    -1,
                )
            }
            PadMode::KeyShift => {
                let semis = KEY_STEPS[i];
                pad(
                    i,
                    key_shift_label(semis),
                    "KEY",
                    true,
                    (s.key_shift - semis).abs() < 0.01,
                    -1,
                )
            }
            PadMode::BeatLoop => {
                let b = pad_beats(i);
                let lit =
                    s.loop_state.active && s.loop_state.beats.is_some_and(|x| (x - b).abs() < 1e-6);
                pad(i, beats_label(b), "BEAT", true, lit, -1)
            }
            PadMode::Slicer => {
                let lit = s.slicer.active
                    && (s.slicer.held == Some(i as u8)
                        || (s.slicer.held.is_none() && s.slicer.current as usize == i));
                pad(i, format!("{}", i + 1), "SLICE", s.slicer.active, lit, -1)
            }
            PadMode::Stems => {
                let part = STEM_PADS[i % 4];
                let name = part.name().to_uppercase();
                if i < 4 {
                    let on = s.stem_levels[part.index()] >= 0.5;
                    pad(
                        i,
                        name,
                        if on { "ON" } else { "MUTED" },
                        s.stems_available,
                        on,
                        -1,
                    )
                } else {
                    let solo = soloed(s) == Some(part);
                    pad(i, name, "SOLO", s.stems_available, solo, -1)
                }
            }
            PadMode::ActiveCensor => {
                pad(i, beats_label(pad_beats(i)), "CENSOR", true, s.censor, -1)
            }
            // Stored sequences are filled in by the sampler (see `sampler::seq_pads`).
            PadMode::SeqCall => pad(i, format!("S{}", i + 1), "", false, false, -1),
            PadMode::Lighting => match i {
                0..=5 => pad(
                    i,
                    format!("{:?}", LIGHT_LOOKS[i]).to_uppercase(),
                    "LOOK",
                    true,
                    false,
                    -1,
                ),
                6 => pad(i, "STROBE", "HOLD", true, false, -1),
                _ => pad(i, "FLASH", "HOLD", true, false, -1),
            },
            PadMode::MemoryCue => match ctx.memory.get(i) {
                Some(&t) => {
                    let lit = (s.cue_point - t).abs() < 0.01;
                    pad(
                        i,
                        format!("M{}", i + 1),
                        crate::format::deck_time(t),
                        true,
                        lit,
                        -1,
                    )
                }
                None => pad(i, format!("M{}", i + 1), "", false, false, -1),
            },
        })
        .collect()
}

/// Commands for pad `index` pressed or released in the deck's mode.
pub(crate) fn press_commands(
    s: &DeckState,
    ctx: &PadCtx,
    index: usize,
    pressed: bool,
) -> Vec<Command> {
    let d = ctx.deck;
    let i = index.min(7);
    let deck = |c: DeckCommand| Command::Deck(d, c);
    match ctx.mode {
        PadMode::HotCue => vec![deck(DeckCommand::HotCue { slot: i, pressed })],
        PadMode::PadFx1 | PadMode::PadFx2 => {
            let page = usize::from(ctx.mode == PadMode::PadFx2);
            vec![Command::PadFx(
                d,
                if pressed {
                    PadFxCommand::Press(crate::fx::pad_fx_preset(page, i))
                } else {
                    PadFxCommand::Release
                },
            )]
        }
        PadMode::BeatJump if pressed => vec![deck(DeckCommand::BeatJump { beats: pad_jump(i) })],
        PadMode::Sampler => {
            let slot = sampler_slot(d, ctx.sampler_bank, i);
            vec![Command::Sampler(if pressed {
                SamplerCommand::Trigger { slot }
            } else {
                SamplerCommand::Release { slot }
            })]
        }
        PadMode::Keyboard => vec![deck(DeckCommand::Keyboard {
            slot: ctx.keyboard_cue.min(HOT_CUES - 1),
            semitones: KEY_STEPS[i],
            pressed,
        })],
        PadMode::KeyShift if pressed => vec![deck(DeckCommand::SetKeyShift(KEY_STEPS[i]))],
        PadMode::BeatLoop if pressed => {
            let b = pad_beats(i);
            let same =
                s.loop_state.active && s.loop_state.beats.is_some_and(|x| (x - b).abs() < 1e-6);
            vec![deck(if same {
                DeckCommand::LoopExit
            } else {
                DeckCommand::AutoLoop { beats: b }
            })]
        }
        PadMode::Stems if pressed && s.stems_available => {
            let part = STEM_PADS[i % 4];
            let set =
                |p: od_engine::Stem, level: f32| deck(DeckCommand::SetStemLevel { stem: p, level });
            if i < 4 {
                let on = s.stem_levels[part.index()] >= 0.5;
                vec![set(part, if on { 0.0 } else { 1.0 })]
            } else if soloed(s) == Some(part) {
                STEM_PADS.iter().map(|p| set(*p, 1.0)).collect()
            } else {
                STEM_PADS
                    .iter()
                    .map(|p| set(*p, if *p == part { 1.0 } else { 0.0 }))
                    .collect()
            }
        }
        PadMode::ActiveCensor if pressed => vec![deck(DeckCommand::CensorFor {
            beats: pad_beats(i),
        })],
        PadMode::MemoryCue if pressed && i < ctx.memory.len() => vec![deck(
            DeckCommand::MemoryCue(od_engine::MemoryCueJump::Index(i)),
        )],
        PadMode::Slicer => {
            let mut v = Vec::new();
            if pressed && !s.slicer.active {
                v.push(deck(DeckCommand::Slicer(SlicerCommand::Enable {
                    beats: ctx.slicer_beats,
                    loop_mode: false,
                })));
            }
            v.push(deck(DeckCommand::Slicer(SlicerCommand::Pad {
                slice: i,
                pressed,
            })));
            v
        }
        _ => Vec::new(),
    }
}

/// The only audible part, if exactly one is.
fn soloed(s: &DeckState) -> Option<od_engine::Stem> {
    let on: Vec<od_engine::Stem> = STEM_PADS
        .iter()
        .copied()
        .filter(|p| s.stem_levels[p.index()] >= 0.5)
        .collect();
    (on.len() == 1).then(|| on[0])
}

/// Commands when a deck changes from `old` to `new` pad mode: the slicer
/// starts and stops with its mode; a held Pad FX is let go.
pub(crate) fn mode_change_commands(
    deck: usize,
    old: PadMode,
    new: PadMode,
    slicer_beats: f64,
) -> Vec<Command> {
    let mut v = Vec::new();
    if old == new {
        return v;
    }
    if old == PadMode::Slicer {
        v.push(Command::Deck(
            deck,
            DeckCommand::Slicer(SlicerCommand::Disable),
        ));
    }
    if matches!(old, PadMode::PadFx1 | PadMode::PadFx2) {
        v.push(Command::PadFx(deck, PadFxCommand::Release));
    }
    if new == PadMode::Slicer {
        v.push(Command::Deck(
            deck,
            DeckCommand::Slicer(SlicerCommand::Enable {
                beats: slicer_beats,
                loop_mode: false,
            }),
        ));
    }
    v
}

/// Next hot cue bank (wrapping).
pub(crate) fn step_hot_cue_bank(bank: usize, delta: i32) -> usize {
    (bank as i32 + delta).rem_euclid(HOT_CUE_BANKS as i32) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_engine::{HotCue, LoopState, SAMPLER_TOTAL};

    fn ctx<'a>(mode: PadMode, labels: &'a [String], slots: &'a [Option<SlotInfo>]) -> PadCtx<'a> {
        PadCtx {
            deck: 1,
            mode,
            sampler_bank: 0,
            keyboard_cue: 0,
            slicer_beats: 8.0,
            labels,
            sampler_slots: slots,
            memory: &[],
        }
    }

    #[test]
    fn letters_banks_and_labels() {
        assert_eq!(hot_letter(0), "A");
        assert_eq!(hot_letter(25), "Z");
        assert_eq!(hot_letter(26), "AA");
        assert_eq!(bank_label(0), "A–H");
        assert_eq!(bank_label(1), "I–P");
        assert_eq!(bank_label(3), "Y–AF");
        assert_eq!(key_shift_label(0.2), "±0");
        assert_eq!(key_shift_label(2.0), "+2");
        assert_eq!(key_shift_label(-3.0), "−3");
        assert_eq!(step_hot_cue_bank(0, -1), 3);
        assert_eq!(step_in(&JUMP_SIZES, 4.0, 1), 8.0);
        assert_eq!(step_in(&JUMP_SIZES, 64.0, 1), 64.0);
        assert_eq!(step_in(&SLICER_BEATS, 8.0, -1), 4.0);
    }

    #[test]
    fn modes_cycle_on_screen() {
        assert_eq!(step_mode(PadMode::HotCue, 1), PadMode::PadFx1);
        assert_eq!(step_mode(PadMode::HotCue, -1), PadMode::SeqCall);
        assert_eq!(step_mode(PadMode::PadFx2, 1), PadMode::BeatJump);
        assert_eq!(step_mode(PadMode::Slicer, 1), PadMode::Stems);
        assert_eq!(step_mode(PadMode::MemoryCue, 1), PadMode::Lighting);
        assert_eq!(step_mode(PadMode::Lighting, 1), PadMode::ActiveCensor);
        assert_eq!(step_mode(PadMode::SeqCall, 1), PadMode::HotCue);
    }

    #[test]
    fn hot_cue_pads_follow_the_bank() {
        let mut s = DeckState {
            hot_cue_bank: 1,
            ..Default::default()
        };
        s.hot_cues[2] = Some(HotCue {
            start: 61.0,
            loop_end: None,
        });
        let mut labels = vec![String::new(); 32];
        labels[10] = "Drop".into();
        let p = pad_views(
            &s,
            &ChannelState::default(),
            &[],
            &ctx(PadMode::HotCue, &labels, &[]),
        );
        assert_eq!(p.len(), 8);
        assert_eq!(
            (
                p[2].caption.as_str(),
                p[2].sub.as_str(),
                p[2].set,
                p[2].cue_slot
            ),
            ("K", "Drop", true, 2)
        );
        assert_eq!((p[0].caption.as_str(), p[0].set), ("I", false));
        assert_eq!(param_label(&s, &ctx(PadMode::HotCue, &labels, &[])), "I–P");
        let c = press_commands(&s, &ctx(PadMode::HotCue, &labels, &[]), 2, true);
        assert!(matches!(
            c[..],
            [Command::Deck(
                1,
                DeckCommand::HotCue {
                    slot: 2,
                    pressed: true
                }
            )]
        ));
    }

    #[test]
    fn sampler_pads_play_the_deck_half_of_the_bank() {
        let mut sampler = vec![SamplerSlotState::default(); SAMPLER_TOTAL];
        sampler[16 + 8 + 3].loaded = true;
        sampler[16 + 8 + 3].playing = true;
        let mut slots = vec![None; SAMPLER_TOTAL];
        slots[27] = Some(SlotInfo {
            name: "Vox".into(),
            ..Default::default()
        });
        let mut c = ctx(PadMode::Sampler, &[], &slots);
        c.sampler_bank = 1;
        let p = pad_views(
            &DeckState::default(),
            &ChannelState::default(),
            &sampler,
            &c,
        );
        assert_eq!(
            (p[3].caption.as_str(), p[3].sub.as_str(), p[3].set, p[3].lit),
            ("12", "Vox", true, true)
        );
        assert_eq!(sampler_slot(0, 1, 3), 19);
        let down = press_commands(&DeckState::default(), &c, 3, true);
        assert!(matches!(
            down[..],
            [Command::Sampler(SamplerCommand::Trigger { slot: 27 })]
        ));
        let up = press_commands(&DeckState::default(), &c, 3, false);
        assert!(matches!(
            up[..],
            [Command::Sampler(SamplerCommand::Release { slot: 27 })]
        ));
    }

    #[test]
    fn jump_loop_key_and_fx_pads() {
        let s = DeckState {
            loop_state: LoopState {
                active: true,
                beats: Some(4.0),
                ..Default::default()
            },
            key_shift: 2.0,
            ..Default::default()
        };
        let none: [String; 0] = [];
        let jump = pad_views(
            &s,
            &ChannelState::default(),
            &[],
            &ctx(PadMode::BeatJump, &none, &[]),
        );
        assert_eq!(
            (jump[0].caption.as_str(), jump[7].caption.as_str()),
            ("−1", "+8")
        );
        assert!(
            matches!(press_commands(&s, &ctx(PadMode::BeatJump, &none, &[]), 5, true)[..], [Command::Deck(1, DeckCommand::BeatJump { beats })] if beats == 2.0)
        );
        assert!(press_commands(&s, &ctx(PadMode::BeatJump, &none, &[]), 5, false).is_empty());
        let lp = pad_views(
            &s,
            &ChannelState::default(),
            &[],
            &ctx(PadMode::BeatLoop, &none, &[]),
        );
        assert!(lp[4].lit && !lp[3].lit);
        // Pressing the running loop's pad ends it; another size starts one.
        assert!(matches!(
            press_commands(&s, &ctx(PadMode::BeatLoop, &none, &[]), 4, true)[..],
            [Command::Deck(1, DeckCommand::LoopExit)]
        ));
        assert!(matches!(
            press_commands(&s, &ctx(PadMode::BeatLoop, &none, &[]), 2, true)[..],
            [Command::Deck(1, DeckCommand::AutoLoop { .. })]
        ));
        let ks = pad_views(
            &s,
            &ChannelState::default(),
            &[],
            &ctx(PadMode::KeyShift, &none, &[]),
        );
        assert!(ks[5].lit && ks[5].caption == "+2");
        assert!(
            matches!(press_commands(&s, &ctx(PadMode::KeyShift, &none, &[]), 0, true)[..], [Command::Deck(1, DeckCommand::SetKeyShift(x))] if x == -3.0)
        );
        let kb = press_commands(&s, &ctx(PadMode::Keyboard, &none, &[]), 6, true);
        assert!(
            matches!(kb[..], [Command::Deck(1, DeckCommand::Keyboard { slot: 0, semitones, pressed: true })] if semitones == 3.0)
        );
        let fx = press_commands(&s, &ctx(PadMode::PadFx2, &none, &[]), 7, true);
        assert!(matches!(
            fx[..],
            [Command::PadFx(1, PadFxCommand::Press(_))]
        ));
        assert!(matches!(
            press_commands(&s, &ctx(PadMode::PadFx1, &none, &[]), 7, false)[..],
            [Command::PadFx(1, PadFxCommand::Release)]
        ));
    }

    #[test]
    fn slicer_starts_with_its_mode() {
        let none: [String; 0] = [];
        let s = DeckState::default();
        let c = press_commands(&s, &ctx(PadMode::Slicer, &none, &[]), 1, true);
        assert_eq!(c.len(), 2, "enables first when off");
        let v = mode_change_commands(0, PadMode::HotCue, PadMode::Slicer, 8.0);
        assert!(
            matches!(v[..], [Command::Deck(0, DeckCommand::Slicer(SlicerCommand::Enable { beats, .. }))] if beats == 8.0)
        );
        let v = mode_change_commands(0, PadMode::Slicer, PadMode::PadFx1, 8.0);
        assert!(matches!(
            v[..],
            [Command::Deck(
                0,
                DeckCommand::Slicer(SlicerCommand::Disable)
            )]
        ));
        assert_eq!(
            mode_change_commands(0, PadMode::PadFx1, PadMode::HotCue, 8.0).len(),
            1
        );
        assert!(mode_change_commands(0, PadMode::HotCue, PadMode::HotCue, 8.0).is_empty());
    }

    #[test]
    fn stems_pads_mute_and_solo() {
        let mut s = DeckState {
            stems_available: true,
            stem_levels: [1.0; 4],
            ..Default::default()
        };
        let c = ctx(PadMode::Stems, &[], &[]);
        let v = pad_views(&s, &ChannelState::default(), &[], &c);
        assert_eq!((v[0].caption.as_str(), v[0].lit), ("VOCALS", true));
        // A mutes vocals.
        let cmds = press_commands(&s, &c, 0, true);
        assert!(
            matches!(cmds[..], [Command::Deck(1, DeckCommand::SetStemLevel { stem: od_engine::Stem::Vocals, level })] if level == 0.0)
        );
        // G solos drums: the other parts go to 0.
        let cmds = press_commands(&s, &c, 6, true);
        assert_eq!(cmds.len(), 4);
        s.stem_levels = [0.0, 1.0, 0.0, 0.0]; // drums only (index 1)
        assert!(
            pad_views(&s, &ChannelState::default(), &[], &c)[6].lit,
            "drums solo shown"
        );
        // Pressing the solo again brings everything back.
        assert!(press_commands(&s, &c, 6, true).iter().all(|c| matches!(c, Command::Deck(_, DeckCommand::SetStemLevel { level, .. }) if *level == 1.0)));
        // No parts: nothing happens.
        s.stems_available = false;
        assert!(press_commands(&s, &c, 0, true).is_empty());
        assert_eq!(param_label(&s, &c), "NO PARTS");
    }

    #[test]
    fn memory_cue_pads_call_cues_in_order() {
        let memory = [12.0, 64.5];
        let s = DeckState {
            cue_point: 64.5,
            ..Default::default()
        };
        let c = PadCtx {
            memory: &memory,
            ..ctx(PadMode::MemoryCue, &[], &[])
        };
        let v = pad_views(&s, &ChannelState::default(), &[], &c);
        assert_eq!((v[0].set, v[1].lit, v[2].set), (true, true, false));
        assert!(matches!(
            press_commands(&s, &c, 1, true)[..],
            [Command::Deck(
                1,
                DeckCommand::MemoryCue(od_engine::MemoryCueJump::Index(1))
            )]
        ));
        assert!(press_commands(&s, &c, 5, true).is_empty());
        assert_eq!(param_label(&s, &c), "2 CUES");
    }

    #[test]
    fn active_censor_pads_censor_for_their_beats() {
        let s = DeckState::default();
        let c = ctx(PadMode::ActiveCensor, &[], &[]);
        assert_eq!(
            pad_views(&s, &ChannelState::default(), &[], &c)[4].caption,
            "4"
        );
        assert!(
            matches!(press_commands(&s, &c, 4, true)[..], [Command::Deck(1, DeckCommand::CensorFor { beats })] if beats == 4.0)
        );
        assert!(press_commands(&s, &c, 4, false).is_empty());
    }
}
