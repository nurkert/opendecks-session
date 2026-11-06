//! Location of a track's analysis files on the stick.
//!
//! rekordbox stores the analysis of a track in
//! `/PIONEER/USBANLZ/P<xxx>/<yyyyyyyy>/ANLZ0000.DAT` where both directory
//! names are derived from the USB-relative audio path. Players are reported
//! to compute this directory themselves from the audio path instead of
//! trusting the `analyze_path` column, so the exact function matters.
//!
//! The function was recovered by the `fourfour` project and published (with
//! verification against a 587-track rekordbox export) in the MIT licensed
//! `baken-export` crate (`src/anlz/hash.rs`). This is an independent
//! implementation of that description; it is verified in the tests against
//! the directories of rekordbox's demo-track export.
//!
//! Over the UTF-16 code units `c` of the path, starting with `h = 0`:
//! `h = h * 0x5BC9 + c; h = h * 0x93B5 + c` (wrapping 32 bit). The
//! second directory is `r = h mod 200003` as 8 hex digits, the first is
//! `P` + 3 hex digits of seven bits gathered from `r`.

use std::collections::HashMap;

const MODULUS: u32 = 200_003;

/// `(p, r)` for a USB-relative audio path such as `/Contents/A/B/x.mp3`.
pub fn anlz_hash(device_path: &str) -> (u32, u32) {
    let h = device_path.encode_utf16().fold(0u32, |h, unit| {
        let c = u32::from(unit);
        let h = h.wrapping_mul(0x5BC9).wrapping_add(c);
        h.wrapping_mul(0x93B5).wrapping_add(c)
    });
    let r = h % MODULUS;
    // (source bit, destination bit) pairs.
    const BITS: [(u32, u32); 7] = [(0, 0), (2, 1), (6, 2), (7, 3), (9, 4), (13, 5), (16, 6)];
    let p = BITS
        .iter()
        .fold(0, |p, &(src, dst)| p | (((r >> src) & 1) << dst));
    (p, r)
}

/// Directory holding the analysis files of `device_path`, e.g.
/// `/PIONEER/USBANLZ/P016/0000875E`.
pub fn anlz_dir_for(device_path: &str) -> String {
    let (p, r) = anlz_hash(device_path);
    format!("/PIONEER/USBANLZ/P{p:03X}/{r:08X}")
}

/// Path of the `.DAT` file for a track that does not share its directory
/// with another track (`…/ANLZ0000.DAT`). Use [`AnlzSlots`] for a whole
/// export so that hash collisions get `ANLZ0001`, `ANLZ0002`, ….
pub fn anlz_dat_path_for(device_path: &str) -> String {
    format!("{}/ANLZ0000.DAT", anlz_dir_for(device_path))
}

/// Hands out `.DAT` paths in export order. Since the hash is taken modulo
/// 200003, two tracks can land in the same directory; rekordbox then numbers
/// the files `ANLZ0000`, `ANLZ0001`, … in export order.
#[derive(Debug, Default, Clone)]
pub struct AnlzSlots {
    used: HashMap<String, u32>,
}

impl AnlzSlots {
    pub fn new() -> Self {
        Self::default()
    }

    /// `.DAT` path for the next track; `.EXT` / `.2EX` share the stem.
    pub fn assign(&mut self, device_path: &str) -> String {
        let dir = anlz_dir_for(device_path);
        let n = self.used.entry(dir.clone()).or_insert(0);
        let path = format!("{dir}/ANLZ{:04}.DAT", *n);
        *n += 1;
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rekordbox_demo_export() {
        // Directories rekordbox chose for its bundled demo tracks.
        assert_eq!(
            anlz_dir_for("/Contents/Loopmasters/UnknownAlbum/Demo Track 1.mp3"),
            "/PIONEER/USBANLZ/P016/0000875E"
        );
        assert_eq!(
            anlz_dir_for("/Contents/Loopmasters/UnknownAlbum/Demo Track 2.mp3"),
            "/PIONEER/USBANLZ/P053/0001D21F"
        );
    }

    #[test]
    fn slots_number_collisions() {
        let mut s = AnlzSlots::new();
        let a = s.assign("/Contents/a.mp3");
        let b = s.assign("/Contents/a.mp3");
        assert!(a.ends_with("ANLZ0000.DAT"));
        assert!(b.ends_with("ANLZ0001.DAT"));
        assert_eq!(a[..a.len() - 12], b[..b.len() - 12]);
    }
}
