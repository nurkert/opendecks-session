//! Optional per-device Rust hooks for behaviour a TOML mapping cannot express.
//!
//! A mapping selects a module with `device.module = "<id>"`. The module sees
//! every raw message before the mapping (and may consume it), and every
//! action after the mapping (and may rewrite, drop or add actions).

use crate::action::{ControlAction, DeckAction, JogMode};
use crate::mapping::Mapping;

/// Context passed to module hooks.
pub struct ModuleContext<'a> {
    pub mapping: &'a Mapping,
}

pub trait DeviceModule: Send {
    fn id(&self) -> &'static str;

    /// Raw input hook. Return `true` to consume the message (the mapping
    /// will not see it). Actions pushed to `out` are emitted.
    fn on_raw(
        &mut self,
        _ctx: &ModuleContext,
        _bytes: &[u8],
        _out: &mut Vec<ControlAction>,
    ) -> bool {
        false
    }

    /// Action hook: push the (possibly transformed) action(s) to `out`.
    fn on_action(
        &mut self,
        _ctx: &ModuleContext,
        action: ControlAction,
        out: &mut Vec<ControlAction>,
    ) {
        out.push(action);
    }
}

/// Module that does nothing; used when a mapping names none.
#[derive(Debug, Default)]
pub struct NoModule;

impl DeviceModule for NoModule {
    fn id(&self) -> &'static str {
        "none"
    }
}

/// Shared behaviour for Pioneer DDJ controllers.
///
/// - Tempo centre detent: positions within `tempo_detent` of the centre snap
///   to exactly 0 so "tempo 0 %" is reachable despite 14-bit jitter.
/// - Jog search acceleration: shift-jog search ticks are multiplied by
///   `search_gain`, and bend ticks are scaled by `bend_gain`.
#[derive(Debug)]
pub struct PioneerDdj {
    pub tempo_detent: f32,
    pub search_gain: i32,
    pub bend_gain: f32,
    bend_remainder: f32,
}

impl Default for PioneerDdj {
    fn default() -> Self {
        Self {
            tempo_detent: 0.004,
            search_gain: 8,
            bend_gain: 1.0,
            bend_remainder: 0.0,
        }
    }
}

impl DeviceModule for PioneerDdj {
    fn id(&self) -> &'static str {
        "pioneer-ddj"
    }

    fn on_action(
        &mut self,
        _ctx: &ModuleContext,
        action: ControlAction,
        out: &mut Vec<ControlAction>,
    ) {
        match action {
            ControlAction::Deck {
                deck,
                action: DeckAction::Tempo(v),
            } => {
                let v = if v.abs() < self.tempo_detent { 0.0 } else { v };
                out.push(ControlAction::Deck {
                    deck,
                    action: DeckAction::Tempo(v),
                });
            }
            ControlAction::Deck {
                deck,
                action: DeckAction::JogTurn { ticks, mode },
            } => {
                let ticks = match mode {
                    JogMode::Search => ticks * self.search_gain,
                    JogMode::Bend => {
                        let f = ticks as f32 * self.bend_gain + self.bend_remainder;
                        let t = f.trunc();
                        self.bend_remainder = f - t;
                        t as i32
                    }
                    JogMode::Scratch => ticks,
                };
                if ticks != 0 {
                    out.push(ControlAction::Deck {
                        deck,
                        action: DeckAction::JogTurn { ticks, mode },
                    });
                }
            }
            a => out.push(a),
        }
    }
}

/// Instantiates the module a mapping asks for.
pub fn module_for(id: Option<&str>) -> Box<dyn DeviceModule> {
    match id {
        Some("pioneer-ddj") => Box::new(PioneerDdj::default()),
        Some(other) => {
            tracing::warn!("unknown device module `{other}`, using none");
            Box::new(NoModule)
        }
        None => Box::new(NoModule),
    }
}
