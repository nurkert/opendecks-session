//! DeviceSQL strings (Deep Symmetry, "DeviceSQL Strings").
//!
//! * Short ASCII: one byte `((len + 1) << 1) | 1` followed by `len` bytes
//!   (no terminator), at most 126 bytes of text.
//! * Long ASCII: `0x40`, u16 total length, pad byte, text.
//! * Long UTF-16LE: `0x90`, u16 total length, pad byte, text.
//!
//! rekordbox writes ASCII text as short strings and everything else as
//! UTF-16LE; the long ASCII form is not produced by rekordbox exports, so
//! ASCII text longer than 126 bytes is written as UTF-16LE here as well.
//! UTF-16 strings must start on a 4 byte boundary of the row (a stick with
//! unaligned UTF-16 strings froze a CDJ-2000NXS2, see baken-export #116).

use crate::bytes::*;
use crate::{Error, Result};

/// Encoded DeviceSQL string.
pub(crate) struct DString {
    pub bytes: Vec<u8>,
    pub wide: bool,
}

pub(crate) fn encode(s: &str) -> DString {
    if s.is_ascii() && s.len() <= 126 {
        let mut bytes = Vec::with_capacity(s.len() + 1);
        bytes.push((((s.len() + 1) << 1) | 1) as u8);
        bytes.extend_from_slice(s.as_bytes());
        DString { bytes, wide: false }
    } else {
        encode_wide(s)
    }
}

/// Always the UTF-16LE form (used by the `columns` table).
pub(crate) fn encode_wide(s: &str) -> DString {
    let units: Vec<u16> = s.encode_utf16().collect();
    // The total length is a u16; truncate absurdly long strings.
    let max_units = (u16::MAX as usize - 4) / 2;
    let units = &units[..units.len().min(max_units)];
    let mut bytes = Vec::with_capacity(4 + units.len() * 2);
    bytes.push(0x90);
    put_le16(&mut bytes, (4 + units.len() * 2) as u16);
    bytes.push(0);
    for u in units {
        put_le16(&mut bytes, *u);
    }
    DString { bytes, wide: true }
}

/// Decodes the string at `at`. Returns the text and its encoded length.
pub(crate) fn decode(data: &[u8], at: usize) -> Result<(String, usize)> {
    let kind = u8_at(data, at)?;
    if kind & 1 == 1 {
        let len = (kind >> 1) as usize;
        if len == 0 {
            return Ok((String::new(), 1));
        }
        let text = slice(data, at + 1, len - 1)?;
        return Ok((text.iter().map(|&b| b as char).collect(), len));
    }
    let len = le16(data, at + 1)? as usize;
    if len < 4 {
        return Err(Error::Invalid(format!(
            "DeviceSQL string length {len} at {at:#x}"
        )));
    }
    let body = slice(data, at + 4, len - 4)?;
    let text = match kind {
        0x40 => body.iter().map(|&b| b as char).collect(),
        0x90 => {
            // ISRC strings are tagged 0x90 but hold `03` + ASCII + NUL.
            if body.first() == Some(&0x03) && body.last() == Some(&0) && body.len() % 2 == 1 {
                body[1..body.len() - 1].iter().map(|&b| b as char).collect()
            } else {
                let units: Vec<u16> = body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect();
                String::from_utf16_lossy(&units)
            }
        }
        0x20 => String::from_utf8_lossy(body).into_owned(),
        _ => {
            return Err(Error::Invalid(format!(
                "unknown DeviceSQL string kind {kind:#x} at {at:#x}"
            )));
        }
    };
    Ok((text, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_ascii() {
        let d = encode("Red");
        assert_eq!(d.bytes, [0x09, b'R', b'e', b'd']);
        assert_eq!(encode("").bytes, [0x03]);
        assert_eq!(decode(&d.bytes, 0).unwrap(), ("Red".into(), 4));
    }

    #[test]
    fn wide_and_long() {
        let d = encode("Mädchen");
        assert!(d.wide);
        assert_eq!(d.bytes[0], 0x90);
        assert_eq!(decode(&d.bytes, 0).unwrap().0, "Mädchen");
        let long = "x".repeat(200);
        let d = encode(&long);
        assert!(d.wide);
        assert_eq!(decode(&d.bytes, 0).unwrap().0, long);
        let d = encode(&"y".repeat(126));
        assert!(!d.wide);
        assert_eq!(d.bytes[0], 0xff);
    }

    #[test]
    fn long_ascii_is_read() {
        let b = [0x40, 0x07, 0x00, 0x00, b'a', b'b', b'c'];
        assert_eq!(decode(&b, 0).unwrap(), ("abc".into(), 7));
    }
}
