//! `/PIONEER/MYSETTING.DAT`: the player preferences ("My Settings") a player
//! takes over when the stick is inserted.
//!
//! Layout as publicly documented by the pyrekordbox project
//! (`docs/source/formats/mysetting.md`,
//! <https://github.com/dylanljones/pyrekordbox>) and the `setting` module
//! notes of `rekordcrate` (<https://github.com/holzhaus/rekordcrate>):
//!
//! | Offset | Size | Field |
//! |---|---|---|
//! | 0 | 4 | `len_strings` = 96 (u32 LE) |
//! | 4 | 32 | brand, ASCII, zero padded |
//! | 36 | 32 | software, ASCII, zero padded |
//! | 68 | 32 | version, ASCII, zero padded |
//! | 100 | 4 | `len_data` = 40 (u32 LE) |
//! | 104 | 40 | data: one byte per setting, enum values from `0x80` |
//! | 144 | 2 | checksum: CRC-16/XMODEM over bytes 0..144 (u16 LE) |
//! | 146 | 2 | `0x0000` |
//!
//! Setting bytes are numbered from 1 within the data block (as in the
//! documentation and in `od-import-rekordbox`'s reader). Bytes the
//! documentation lists as unknown are written as zero; the first eight
//! bytes hold the documented constant prefix.

/// Length of the string block.
pub const LEN_STRINGS: u32 = 96;
/// Length of the MYSETTING data block.
pub const LEN_DATA: u32 = 40;
/// Total file length.
pub const FILE_LEN: usize = 104 + LEN_DATA as usize + 4;

const BRAND: &str = "PioneerDJ";
const SOFTWARE: &str = "rekordbox";
const VERSION: &str = "0.001";
/// The constant first eight data bytes.
const PREFIX: [u8; 8] = [0x78, 0x56, 0x34, 0x12, 0x02, 0x00, 0x00, 0x00];

/// Tempo slider range of the players.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoRange {
    Six,
    Ten,
    Sixteen,
    Wide,
}

/// Player menu language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    English,
    German,
}

/// The settings OpenDecks writes; everything else gets its documented
/// default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerSettings {
    pub quantize: bool,
    pub auto_cue: bool,
    /// Master tempo (key lock) at load.
    pub master_tempo: bool,
    pub tempo_range: TempoRange,
    /// Jog mode: vinyl (`true`) or CDJ.
    pub jog_vinyl: bool,
    /// Time display: remaining (`true`) or elapsed.
    pub time_remaining: bool,
    pub slip_flashing: bool,
    pub language: Language,
}

impl Default for PlayerSettings {
    fn default() -> Self {
        Self {
            quantize: true,
            auto_cue: true,
            master_tempo: false,
            tempo_range: TempoRange::Ten,
            jog_vinyl: true,
            time_remaining: true,
            slip_flashing: true,
            language: Language::English,
        }
    }
}

fn on_off(v: bool) -> u8 {
    if v { 0x81 } else { 0x80 }
}

/// CRC-16/XMODEM (polynomial 0x1021, initial value 0, no reflection).
pub fn crc16_xmodem(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for b in bytes {
        crc ^= (*b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The 40-byte data block.
pub fn data_block(s: &PlayerSettings) -> [u8; LEN_DATA as usize] {
    let mut d = [0u8; LEN_DATA as usize];
    d[..8].copy_from_slice(&PREFIX);
    let mut set = |n: usize, v: u8| d[n - 1] = v;
    set(9, 0x81); // on air display: on
    set(10, 0x83); // LCD brightness 3
    set(11, on_off(s.quantize));
    set(12, 0x88); // auto cue level: memory
    set(
        13,
        match s.language {
            Language::English => 0x81,
            Language::German => 0x83,
        },
    );
    set(15, 0x82); // jog ring brightness: bright
    set(16, 0x81); // jog ring indicator: on
    set(17, on_off(s.slip_flashing));
    set(21, 0x82); // disc slot illumination: bright
    set(22, 0x80); // eject lock: unlocked
    set(23, 0x80); // sync: off
    set(24, 0x81); // play mode: single
    set(25, 0x80); // quantize beat value: 1 beat
    set(26, 0x81); // hot cue auto load: on
    set(27, 0x80); // hot cue colour: off
    set(30, 0x81); // needle lock: locked
    set(33, on_off(s.time_remaining));
    set(34, on_off(s.jog_vinyl));
    set(35, on_off(s.auto_cue));
    set(36, on_off(s.master_tempo));
    set(37, 0x80 + s.tempo_range as u8);
    set(38, 0x80); // phase meter: type 1
    d
}

/// The complete `MYSETTING.DAT`.
pub fn to_bytes(s: &PlayerSettings) -> Vec<u8> {
    let mut out = Vec::with_capacity(FILE_LEN);
    out.extend_from_slice(&LEN_STRINGS.to_le_bytes());
    for text in [BRAND, SOFTWARE, VERSION] {
        let mut field = [0u8; 32];
        field[..text.len()].copy_from_slice(text.as_bytes());
        out.extend_from_slice(&field);
    }
    out.extend_from_slice(&LEN_DATA.to_le_bytes());
    out.extend_from_slice(&data_block(s));
    let crc = crc16_xmodem(&out);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_xmodem_check_value() {
        // The standard check value of CRC-16/XMODEM.
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
        assert_eq!(crc16_xmodem(b""), 0);
    }

    #[test]
    fn file_layout_matches_the_documentation() {
        let b = to_bytes(&PlayerSettings::default());
        assert_eq!(b.len(), FILE_LEN);
        assert_eq!(&b[0..4], &96u32.to_le_bytes());
        assert_eq!(&b[4..13], b"PioneerDJ");
        assert!(b[13..36].iter().all(|x| *x == 0));
        assert_eq!(&b[36..45], b"rekordbox");
        assert_eq!(&b[68..73], b"0.001");
        assert_eq!(&b[100..104], &40u32.to_le_bytes());
        assert_eq!(&b[104..112], &PREFIX);
        let crc = u16::from_le_bytes([b[144], b[145]]);
        assert_eq!(
            crc,
            crc16_xmodem(&b[..144]),
            "checksum over header and data"
        );
        assert_eq!(&b[146..148], &[0, 0]);
    }

    #[test]
    fn settings_land_on_their_bytes() {
        let s = PlayerSettings {
            quantize: false,
            auto_cue: false,
            master_tempo: true,
            tempo_range: TempoRange::Sixteen,
            jog_vinyl: false,
            time_remaining: false,
            slip_flashing: false,
            language: Language::German,
        };
        let d = data_block(&s);
        let byte = |n: usize| d[n - 1];
        assert_eq!((byte(11), byte(13), byte(17)), (0x80, 0x83, 0x80));
        assert_eq!(
            (byte(33), byte(34), byte(35), byte(36), byte(37)),
            (0x80, 0x80, 0x80, 0x81, 0x82)
        );
        let d = data_block(&PlayerSettings::default());
        let byte = |n: usize| d[n - 1];
        assert_eq!(
            (byte(11), byte(35), byte(36), byte(37)),
            (0x81, 0x81, 0x80, 0x81)
        );
        // Every documented setting byte is an enum value (>= 0x80).
        for n in [
            9, 10, 11, 12, 13, 15, 16, 17, 21, 22, 23, 24, 25, 26, 27, 30, 33, 34, 35, 36, 37, 38,
        ] {
            assert!(byte(n) >= 0x80, "byte {n}");
        }
    }
}
