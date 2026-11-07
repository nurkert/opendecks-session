//! MIDI learn: observe the messages a physical control sends and propose a
//! mapping entry for it.
//!
//! Usage: create a [`LearnSession`] for the action to assign, feed it every
//! incoming message while the user moves/presses the control, then call
//! [`LearnSession::suggest`] and turn the result into TOML with
//! [`LearnedInput::to_toml`].

use crate::mapping::{InputMode, RelEncoding};
use crate::midi::{MidiAddr, MidiMessage, MsgKind};
use std::collections::BTreeMap;
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub struct LearnedInput {
    pub addr: MidiAddr,
    pub lsb: Option<u8>,
    pub mode: InputMode,
    pub encoding: Option<RelEncoding>,
    /// Messages observed for the chosen address.
    pub samples: usize,
}

impl LearnedInput {
    /// A `[[input]]` table for a mapping file.
    pub fn to_toml(&self, action: &str, name: &str) -> String {
        let mut s = String::from("[[input]]\n");
        let _ = writeln!(s, "name = {name:?}");
        match self.addr.kind {
            MsgKind::Note => {
                let _ = writeln!(s, "note = 0x{:02X}", self.addr.number);
            }
            MsgKind::Cc => {
                let _ = writeln!(s, "cc = 0x{:02X}", self.addr.number);
            }
        }
        if let Some(l) = self.lsb {
            let _ = writeln!(s, "lsb = 0x{l:02X}");
        }
        let _ = writeln!(s, "channel = {}", self.addr.channel);
        let mode = match self.mode {
            InputMode::Button => "button",
            InputMode::Absolute => "absolute",
            InputMode::Absolute14 => "absolute14",
            InputMode::Relative => "relative",
        };
        let _ = writeln!(s, "mode = \"{mode}\"");
        if let Some(e) = self.encoding {
            let e = match e {
                RelEncoding::Offset64 => "offset64",
                RelEncoding::TwosComplement => "twos_complement",
                RelEncoding::SignMagnitude => "sign_magnitude",
            };
            let _ = writeln!(s, "encoding = \"{e}\"");
        }
        let _ = writeln!(s, "action = {action:?}");
        s
    }
}

#[derive(Debug, Default)]
pub struct LearnSession {
    seen: BTreeMap<MidiAddr, Vec<u8>>,
    order: Vec<MidiAddr>,
}

impl LearnSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.seen.clear();
        self.order.clear();
    }

    /// Feeds one raw message. Non-note/CC messages are ignored.
    pub fn feed(&mut self, bytes: &[u8]) {
        let Some((addr, value)) = MidiMessage::parse(bytes).and_then(|m| m.addr_value()) else {
            return;
        };
        if !self.seen.contains_key(&addr) {
            self.order.push(addr);
        }
        self.seen.entry(addr).or_default().push(value);
    }

    /// Best guess for the control seen so far.
    pub fn suggest(&self) -> Option<LearnedInput> {
        // A note press wins: buttons and pads.
        if let Some(a) = self.order.iter().find(|a| a.kind == MsgKind::Note) {
            return Some(LearnedInput {
                addr: *a,
                lsb: None,
                mode: InputMode::Button,
                encoding: None,
                samples: self.seen[a].len(),
            });
        }
        // 14-bit pair: CC n and n+0x20 on one channel.
        for a in &self.order {
            if a.number < 0x20 {
                let l = MidiAddr::cc(a.channel, a.number + 0x20);
                if self.seen.contains_key(&l) {
                    return Some(LearnedInput {
                        addr: *a,
                        lsb: Some(l.number),
                        mode: InputMode::Absolute14,
                        encoding: None,
                        samples: self.seen[a].len(),
                    });
                }
            }
        }
        // Single CC: the one with the most messages.
        let (addr, values) = self.seen.iter().max_by_key(|(_, v)| v.len())?;
        let (mode, encoding) = classify_cc(values);
        Some(LearnedInput {
            addr: *addr,
            lsb: None,
            mode,
            encoding,
            samples: values.len(),
        })
    }
}

fn classify_cc(values: &[u8]) -> (InputMode, Option<RelEncoding>) {
    if values.iter().all(|v| *v == 0 || *v == 127) && values.contains(&0) {
        return (InputMode::Button, None);
    }
    if values.len() >= 3 {
        if values.iter().all(|v| (54..=74).contains(v) && *v != 64) {
            return (InputMode::Relative, Some(RelEncoding::Offset64));
        }
        if values
            .iter()
            .all(|v| (1..=10).contains(v) || (118..=127).contains(v))
        {
            return (InputMode::Relative, Some(RelEncoding::TwosComplement));
        }
    }
    (InputMode::Absolute, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_note_button() {
        let mut l = LearnSession::new();
        l.feed(&[0x90, 0x0B, 0x7F]);
        l.feed(&[0x90, 0x0B, 0x00]);
        let s = l.suggest().unwrap();
        assert_eq!(s.mode, InputMode::Button);
        assert_eq!(s.addr, MidiAddr::note(0, 0x0B));
        assert!(s.to_toml("deck.play_pause", "Play").contains("note = 0x0B"));
    }

    #[test]
    fn learns_14bit_fader() {
        let mut l = LearnSession::new();
        for v in [10u8, 11, 12] {
            l.feed(&[0xB1, 0x13, v]);
            l.feed(&[0xB1, 0x33, 0x40]);
        }
        let s = l.suggest().unwrap();
        assert_eq!(s.mode, InputMode::Absolute14);
        assert_eq!(s.lsb, Some(0x33));
    }

    #[test]
    fn learns_relative_jog() {
        let mut l = LearnSession::new();
        for v in [65u8, 66, 65, 63] {
            l.feed(&[0xB0, 0x22, v]);
        }
        let s = l.suggest().unwrap();
        assert_eq!(s.mode, InputMode::Relative);
        assert_eq!(s.encoding, Some(RelEncoding::Offset64));
    }

    #[test]
    fn learns_twos_complement_encoder() {
        let mut l = LearnSession::new();
        for v in [1u8, 1, 127, 2] {
            l.feed(&[0xB6, 0x40, v]);
        }
        assert_eq!(
            l.suggest().unwrap().encoding,
            Some(RelEncoding::TwosComplement)
        );
    }
}
