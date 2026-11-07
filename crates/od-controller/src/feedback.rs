//! State the application pushes to a controller (LEDs, meters, pad colours).

use crate::action::{CrossfaderSide, PadMode, ParamTarget};
use od_core::CueColor;
use serde::{Deserialize, Serialize};

/// A light or indicator on a deck.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeckElement {
    Play,
    Cue,
    Sync,
    Master,
    KeyLock,
    KeySync,
    Slip,
    Quantize,
    Reverse,
    Censor,
    Vinyl,
    TempoReset,
    TempoRange,
    LoopIn,
    LoopOut,
    Reloop,
    LoopActive,
    Loaded,
    Shift,
    PadMode(PadMode),
    /// Pad `n` (0-based) of the given pad mode page.
    Pad {
        mode: PadMode,
        index: u8,
    },
    /// Jog ring / platter indicator (level, 0..1 = one revolution).
    JogIndicator,
    /// The deck is the active one on its side (4-deck controllers).
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixerElement {
    /// Headphone cue (PFL) button.
    Cue,
    /// Channel level meter (level).
    Vu,
    CrossfaderAssign(CrossfaderSide),
}

/// Addressable feedback element.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackTarget {
    Deck {
        deck: u8,
        element: DeckElement,
    },
    Mixer {
        channel: u8,
        element: MixerElement,
    },
    MasterVuLeft,
    MasterVuRight,
    MasterCue,
    BeatFxOn {
        unit: u8,
    },
    BeatFxRelease {
        unit: u8,
    },
    /// Colour FX selection button, by effect name.
    ColorFx(String),
    SamplerCue,
    Recording,
}

/// LED rendering state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedState {
    Off,
    On,
    Dim,
    Blink,
    BlinkFast,
}

impl LedState {
    pub fn from_bool(on: bool) -> Self {
        if on { LedState::On } else { LedState::Off }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackValue {
    Led(LedState),
    /// Meter or indicator level, 0.0..=1.0.
    Level(f32),
    /// A coloured element. `None` colour renders as off.
    Color {
        color: Option<CueColor>,
        state: LedState,
    },
}

/// A message from the application to the controller layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlFeedback {
    Set {
        target: FeedbackTarget,
        value: FeedbackValue,
    },
    /// Current engine value of a continuous parameter (action domain), used
    /// for soft takeover after deck layer switches or software changes.
    Parameter { target: ParamTarget, value: f32 },
    /// Forget what was sent and resend every element (e.g. after reconnect).
    ResendAll,
    /// Turn every known element off.
    AllOff,
}

impl ControlFeedback {
    pub fn led(target: FeedbackTarget, on: bool) -> Self {
        ControlFeedback::Set {
            target,
            value: FeedbackValue::Led(LedState::from_bool(on)),
        }
    }

    pub fn led_state(target: FeedbackTarget, state: LedState) -> Self {
        ControlFeedback::Set {
            target,
            value: FeedbackValue::Led(state),
        }
    }

    pub fn level(target: FeedbackTarget, level: f32) -> Self {
        ControlFeedback::Set {
            target,
            value: FeedbackValue::Level(level),
        }
    }

    pub fn deck_led(deck: u8, element: DeckElement, on: bool) -> Self {
        Self::led(FeedbackTarget::Deck { deck, element }, on)
    }

    pub fn channel_vu(channel: u8, level: f32) -> Self {
        Self::level(
            FeedbackTarget::Mixer {
                channel,
                element: MixerElement::Vu,
            },
            level,
        )
    }

    pub fn pad_color(deck: u8, mode: PadMode, index: u8, color: Option<CueColor>) -> Self {
        ControlFeedback::Set {
            target: FeedbackTarget::Deck {
                deck,
                element: DeckElement::Pad { mode, index },
            },
            value: FeedbackValue::Color {
                color,
                state: LedState::On,
            },
        }
    }
}
