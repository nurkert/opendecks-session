use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Major,
    Minor,
}

/// A musical key: tonic pitch class (0 = C, 1 = C#/Db, …, 11 = B) and mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MusicalKey {
    pub tonic: u8,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyNotation {
    #[default]
    Camelot,
    OpenKey,
    Musical,
}

const NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"];

impl MusicalKey {
    pub fn new(tonic: u8, mode: Mode) -> Self {
        Self { tonic: tonic % 12, mode }
    }

    /// Camelot wheel number 1..=12 (A = minor, B = major).
    pub fn camelot_number(self) -> u8 {
        // Major keys: C=8B, G=9B, … (each fifth up is +1).
        let major_tonic = match self.mode {
            Mode::Major => self.tonic,
            Mode::Minor => (self.tonic + 3) % 12, // relative major
        };
        // Position on circle of fifths relative to C.
        let fifths = (major_tonic as u32 * 7) % 12;
        ((fifths + 7) % 12 + 1) as u8
    }

    pub fn format(self, notation: KeyNotation) -> String {
        match notation {
            KeyNotation::Camelot => format!(
                "{}{}",
                self.camelot_number(),
                if self.mode == Mode::Minor { 'A' } else { 'B' }
            ),
            KeyNotation::OpenKey => {
                // Open Key: C major = 1d, A minor = 1m.
                let n = (self.camelot_number() + 4) % 12 + 1;
                format!("{}{}", n, if self.mode == Mode::Minor { 'm' } else { 'd' })
            }
            KeyNotation::Musical => format!(
                "{}{}",
                NAMES[self.tonic as usize],
                if self.mode == Mode::Minor { "m" } else { "" }
            ),
        }
    }

    pub fn from_camelot(number: u8, minor: bool) -> Option<Self> {
        if !(1..=12).contains(&number) {
            return None;
        }
        let fifths = (number as u32 + 12 - 8) % 12; // 8B = C
        let major_tonic = ((fifths * 7) % 12) as u8;
        Some(if minor {
            Self::new((major_tonic + 9) % 12, Mode::Minor)
        } else {
            Self::new(major_tonic, Mode::Major)
        })
    }

    /// Parses Camelot ("8A"), Open Key ("1m") and musical ("Am", "F#", "Bbm",
    /// "A minor") notations.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let lower = s.to_ascii_lowercase();
        let digits: String = lower.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let n: u8 = digits.parse().ok()?;
            let suffix = &lower[digits.len()..];
            return match suffix {
                "a" => Self::from_camelot(n, true),
                "b" => Self::from_camelot(n, false),
                "m" | "d" if (1..=12).contains(&n) => {
                    let camelot = (n + 6) % 12 + 1;
                    Self::from_camelot(camelot, suffix == "m")
                }
                _ => None,
            };
        }
        let mut chars = s.chars();
        let letter = chars.next()?.to_ascii_uppercase();
        let base: i32 = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let rest: String = chars.collect();
        let (acc, rest) = if let Some(r) = rest.strip_prefix('#').or(rest.strip_prefix('♯')) {
            (1, r.to_string())
        } else if let Some(r) = rest.strip_prefix('b').or(rest.strip_prefix('♭')) {
            (-1, r.to_string())
        } else {
            (0, rest)
        };
        let rest = rest.trim().to_ascii_lowercase();
        let mode = match rest.as_str() {
            "" | "maj" | "major" => Mode::Major,
            "m" | "min" | "minor" => Mode::Minor,
            _ => return None,
        };
        Some(Self::new((base + acc).rem_euclid(12) as u8, mode))
    }

    /// Keys that mix harmonically with this one (same, ±1 on the wheel,
    /// relative major/minor).
    pub fn compatible(self) -> [MusicalKey; 4] {
        let n = self.camelot_number();
        let minor = self.mode == Mode::Minor;
        let up = n % 12 + 1;
        let down = (n + 10) % 12 + 1;
        [
            self,
            Self::from_camelot(up, minor).unwrap(),
            Self::from_camelot(down, minor).unwrap(),
            Self::from_camelot(n, !minor).unwrap(),
        ]
    }

    /// Distance on the Camelot wheel (0 = identical, 1 = adjacent or
    /// relative, larger = clashing).
    pub fn camelot_distance(self, other: MusicalKey) -> u8 {
        let a = self.camelot_number() as i32;
        let b = other.camelot_number() as i32;
        let d = (a - b).rem_euclid(12);
        let wheel = d.min(12 - d) as u8;
        wheel + u8::from(self.mode != other.mode)
    }
}

impl fmt::Display for MusicalKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format(KeyNotation::Camelot))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_reference_points() {
        assert_eq!(MusicalKey::new(0, Mode::Major).format(KeyNotation::Camelot), "8B");
        assert_eq!(MusicalKey::new(9, Mode::Minor).format(KeyNotation::Camelot), "8A");
        assert_eq!(MusicalKey::new(7, Mode::Major).format(KeyNotation::Camelot), "9B");
        assert_eq!(MusicalKey::new(5, Mode::Major).format(KeyNotation::Camelot), "7B");
        assert_eq!(MusicalKey::new(11, Mode::Major).format(KeyNotation::Camelot), "1B");
        assert_eq!(MusicalKey::new(8, Mode::Minor).format(KeyNotation::Camelot), "1A");
    }

    #[test]
    fn open_key_reference_points() {
        assert_eq!(MusicalKey::new(0, Mode::Major).format(KeyNotation::OpenKey), "1d");
        assert_eq!(MusicalKey::new(9, Mode::Minor).format(KeyNotation::OpenKey), "1m");
        assert_eq!(MusicalKey::new(7, Mode::Major).format(KeyNotation::OpenKey), "2d");
    }

    #[test]
    fn parse_all_notations_roundtrip() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let k = MusicalKey::new(tonic, mode);
                for n in [KeyNotation::Camelot, KeyNotation::OpenKey, KeyNotation::Musical] {
                    assert_eq!(MusicalKey::parse(&k.format(n)), Some(k), "{k:?} {n:?}");
                }
            }
        }
        assert_eq!(MusicalKey::parse("F# minor"), Some(MusicalKey::new(6, Mode::Minor)));
        assert_eq!(MusicalKey::parse("x"), None);
    }

    #[test]
    fn distances() {
        let am = MusicalKey::parse("8A").unwrap();
        assert_eq!(am.camelot_distance(MusicalKey::parse("9A").unwrap()), 1);
        assert_eq!(am.camelot_distance(MusicalKey::parse("8B").unwrap()), 1);
        assert_eq!(am.camelot_distance(MusicalKey::parse("2A").unwrap()), 6);
        assert!(am.compatible().contains(&MusicalKey::parse("7A").unwrap()));
    }
}
