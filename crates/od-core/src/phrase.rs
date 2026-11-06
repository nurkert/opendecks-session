use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhraseKind {
    Intro,
    Verse,
    Chorus,
    Bridge,
    Up,
    Down,
    Outro,
}

/// Overall character used to colour phrases (mirrors the three moods used by
/// common DJ software: high, mid and low energy).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhraseMood {
    High,
    #[default]
    Mid,
    Low,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    /// Index of the first beat of the phrase in the track's beatgrid.
    pub start_beat: u32,
    /// Index of the beat after the phrase.
    pub end_beat: u32,
    pub kind: PhraseKind,
}
