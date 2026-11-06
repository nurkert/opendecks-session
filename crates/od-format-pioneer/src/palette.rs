//! Colour tables of the Pioneer formats.

use od_core::CueColor;

/// RGB values of the hot cue colour codes `0x01..=0x3e` used in `PCP2`
/// entries (index 0 is code 1). Values as documented by Deep Symmetry
/// (Beat Link `CueList.findRekordboxColor`) and the rekordcrate docs.
pub const HOT_CUE_PALETTE: [CueColor; 62] = {
    const fn c(v: u32) -> CueColor {
        CueColor::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
    [
        c(0x305aff),
        c(0x5073ff),
        c(0x508cff),
        c(0x50a0ff),
        c(0x50b4ff),
        c(0x50b0f2),
        c(0x50aee8),
        c(0x45acdb),
        c(0x00e0ff),
        c(0x19daf0),
        c(0x32d2e6),
        c(0x21b4b9),
        c(0x20aaa0),
        c(0x1fa392),
        c(0x19a08c),
        c(0x14a584),
        c(0x14aa7d),
        c(0x10b176),
        c(0x30d26e),
        c(0x37de5a),
        c(0x3ceb50),
        c(0x28e214),
        c(0x7dc13d),
        c(0x8cc832),
        c(0x9bd723),
        c(0xa5e116),
        c(0xa5dc0a),
        c(0xaad208),
        c(0xb4c805),
        c(0xb4be04),
        c(0xbab404),
        c(0xc3af04),
        c(0xe1aa00),
        c(0xffa000),
        c(0xff9600),
        c(0xff8c00),
        c(0xff7500),
        c(0xe0641b),
        c(0xe0461e),
        c(0xe0301e),
        c(0xe02823),
        c(0xe62828),
        c(0xff376f),
        c(0xff2d6f),
        c(0xff127b),
        c(0xf51e8c),
        c(0xeb2da0),
        c(0xe637b4),
        c(0xde44cf),
        c(0xde448d),
        c(0xe630b4),
        c(0xe619dc),
        c(0xe600ff),
        c(0xdc00ff),
        c(0xcc00ff),
        c(0xb432ff),
        c(0xb93cff),
        c(0xc542ff),
        c(0xaa5aff),
        c(0xaa72ff),
        c(0x8272ff),
        c(0x6473ff),
    ]
};

/// The eight track / memory cue colours of the `colors` table, ids 1..=8
/// (Pink, Red, Orange, Yellow, Green, Aqua, Blue, Purple). The players show
/// fixed colours for these ids; the RGB values here are approximations used
/// to map arbitrary colours to the nearest id and back.
pub const TRACK_COLORS: [(&str, CueColor); 8] = [
    ("Pink", CueColor::rgb(0xf8, 0x70, 0xf8)),
    ("Red", CueColor::rgb(0xf8, 0x2c, 0x30)),
    ("Orange", CueColor::rgb(0xf8, 0x80, 0x00)),
    ("Yellow", CueColor::rgb(0xf8, 0xe3, 0x31)),
    ("Green", CueColor::rgb(0x10, 0xe0, 0x20)),
    ("Aqua", CueColor::rgb(0x10, 0xb0, 0xf8)),
    ("Blue", CueColor::rgb(0x30, 0x5a, 0xff)),
    ("Purple", CueColor::rgb(0x91, 0x48, 0xfc)),
];

/// Colour id 1..=8 nearest to `c`.
pub(crate) fn track_color_id(c: CueColor) -> u8 {
    let pal: Vec<CueColor> = TRACK_COLORS.iter().map(|(_, c)| *c).collect();
    c.nearest(&pal) as u8 + 1
}

pub(crate) fn track_color(id: u8) -> Option<CueColor> {
    TRACK_COLORS
        .get((id as usize).checked_sub(1)?)
        .map(|(_, c)| *c)
}

/// Hot cue colour code 1..=62 nearest to `c`.
pub(crate) fn hot_cue_code(c: CueColor) -> u8 {
    c.nearest(&HOT_CUE_PALETTE) as u8 + 1
}

pub(crate) fn hot_cue_color(code: u8) -> Option<CueColor> {
    HOT_CUE_PALETTE
        .get((code as usize).checked_sub(1)?)
        .copied()
}
