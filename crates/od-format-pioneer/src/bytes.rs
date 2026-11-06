//! Bounds-checked byte access helpers.

use crate::{Error, Result};

/// Returns `data[at..at + len]` or a [`Error::Truncated`].
pub(crate) fn slice(data: &[u8], at: usize, len: usize) -> Result<&[u8]> {
    at.checked_add(len)
        .and_then(|end| data.get(at..end))
        .ok_or(Error::Truncated(at))
}

pub(crate) fn u8_at(data: &[u8], at: usize) -> Result<u8> {
    data.get(at).copied().ok_or(Error::Truncated(at))
}

pub(crate) fn be16(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_be_bytes(slice(data, at, 2)?.try_into().unwrap()))
}

pub(crate) fn be32(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_be_bytes(slice(data, at, 4)?.try_into().unwrap()))
}

pub(crate) fn le16(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(slice(data, at, 2)?.try_into().unwrap()))
}

pub(crate) fn le32(data: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(data, at, 4)?.try_into().unwrap()))
}

pub(crate) fn put_be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_le16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_le32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub(crate) fn set_le16(out: &mut [u8], at: usize, v: u16) {
    out[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn set_le32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn align4(n: usize) -> usize {
    n.next_multiple_of(4)
}

/// UTF-16BE with a trailing NUL, as used by `PPTH` and cue comments.
pub(crate) fn utf16be_nul(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_be_bytes)
        .collect()
}

/// Decodes UTF-16BE, stopping at the first NUL.
pub(crate) fn decode_utf16be(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}
