//! Raw MIDI message decoding.

use std::fmt;

/// A decoded MIDI message. Channels are wire values `0..=15`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MidiMessage {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    ControlChange {
        channel: u8,
        control: u8,
        value: u8,
    },
    PolyPressure {
        channel: u8,
        note: u8,
        value: u8,
    },
    ProgramChange {
        channel: u8,
        program: u8,
    },
    ChannelPressure {
        channel: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        value: u16,
    },
    SysEx(Vec<u8>),
    /// System common / realtime or anything we do not interpret.
    Other(Vec<u8>),
}

/// Kind of addressable channel-voice message used as a mapping key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MsgKind {
    Note,
    Cc,
}

/// Address of a note or controller on a given channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MidiAddr {
    pub kind: MsgKind,
    pub channel: u8,
    pub number: u8,
}

impl MidiAddr {
    pub const fn note(channel: u8, number: u8) -> Self {
        Self {
            kind: MsgKind::Note,
            channel,
            number,
        }
    }
    pub const fn cc(channel: u8, number: u8) -> Self {
        Self {
            kind: MsgKind::Cc,
            channel,
            number,
        }
    }

    /// Bytes for a message to this address with `value` (note on / CC).
    pub fn message(self, value: u8) -> [u8; 3] {
        let status = match self.kind {
            MsgKind::Note => 0x90,
            MsgKind::Cc => 0xB0,
        } | (self.channel & 0x0F);
        [status, self.number & 0x7F, value & 0x7F]
    }
}

impl fmt::Display for MidiAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let k = match self.kind {
            MsgKind::Note => "note",
            MsgKind::Cc => "cc",
        };
        write!(f, "{k} ch{} 0x{:02X}", self.channel, self.number)
    }
}

impl MidiMessage {
    /// Decodes one complete MIDI message. Returns `None` for empty input or
    /// truncated channel messages.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let status = *bytes.first()?;
        if status == 0xF0 {
            return Some(MidiMessage::SysEx(bytes.to_vec()));
        }
        if !(0x80..0xF0).contains(&status) {
            return Some(MidiMessage::Other(bytes.to_vec()));
        }
        let channel = status & 0x0F;
        let d1 = bytes.get(1).copied();
        let d2 = bytes.get(2).copied();
        Some(match status & 0xF0 {
            0x80 => MidiMessage::NoteOff {
                channel,
                note: d1?,
                velocity: d2?,
            },
            0x90 => {
                let (note, velocity) = (d1?, d2?);
                if velocity == 0 {
                    MidiMessage::NoteOff {
                        channel,
                        note,
                        velocity: 0,
                    }
                } else {
                    MidiMessage::NoteOn {
                        channel,
                        note,
                        velocity,
                    }
                }
            }
            0xA0 => MidiMessage::PolyPressure {
                channel,
                note: d1?,
                value: d2?,
            },
            0xB0 => MidiMessage::ControlChange {
                channel,
                control: d1?,
                value: d2?,
            },
            0xC0 => MidiMessage::ProgramChange {
                channel,
                program: d1?,
            },
            0xD0 => MidiMessage::ChannelPressure {
                channel,
                value: d1?,
            },
            0xE0 => MidiMessage::PitchBend {
                channel,
                value: (d1? as u16) | ((d2? as u16) << 7),
            },
            _ => unreachable!(),
        })
    }

    /// Mapping address and 7-bit value, for notes and CCs.
    pub fn addr_value(&self) -> Option<(MidiAddr, u8)> {
        match *self {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } => Some((MidiAddr::note(channel, note), velocity)),
            MidiMessage::NoteOff { channel, note, .. } => Some((MidiAddr::note(channel, note), 0)),
            MidiMessage::ControlChange {
                channel,
                control,
                value,
            } => Some((MidiAddr::cc(channel, control), value)),
            _ => None,
        }
    }
}

impl fmt::Display for MidiMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MidiMessage::NoteOn {
                channel,
                note,
                velocity,
            } => {
                write!(f, "NoteOn  ch{channel:<2} note 0x{note:02X} vel {velocity}")
            }
            MidiMessage::NoteOff {
                channel,
                note,
                velocity,
            } => {
                write!(f, "NoteOff ch{channel:<2} note 0x{note:02X} vel {velocity}")
            }
            MidiMessage::ControlChange {
                channel,
                control,
                value,
            } => {
                write!(f, "CC      ch{channel:<2} cc   0x{control:02X} val {value}")
            }
            MidiMessage::PolyPressure {
                channel,
                note,
                value,
            } => {
                write!(f, "PolyAT  ch{channel:<2} note 0x{note:02X} val {value}")
            }
            MidiMessage::ProgramChange { channel, program } => {
                write!(f, "Program ch{channel:<2} {program}")
            }
            MidiMessage::ChannelPressure { channel, value } => {
                write!(f, "ChanAT  ch{channel:<2} val {value}")
            }
            MidiMessage::PitchBend { channel, value } => {
                write!(f, "Bend    ch{channel:<2} {value}")
            }
            MidiMessage::SysEx(b) => write!(f, "SysEx   {}", hex(b)),
            MidiMessage::Other(b) => write!(f, "Other   {}", hex(b)),
        }
    }
}

/// Formats bytes as space separated upper-case hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_on_zero_is_note_off() {
        assert_eq!(
            MidiMessage::parse(&[0x91, 0x0B, 0x00]),
            Some(MidiMessage::NoteOff {
                channel: 1,
                note: 0x0B,
                velocity: 0
            })
        );
    }

    #[test]
    fn cc_roundtrip() {
        let a = MidiAddr::cc(6, 0x1F);
        let m = MidiMessage::parse(&a.message(99)).unwrap();
        assert_eq!(m.addr_value(), Some((a, 99)));
    }

    #[test]
    fn truncated_is_none() {
        assert_eq!(MidiMessage::parse(&[0xB0, 0x01]), None);
    }
}
