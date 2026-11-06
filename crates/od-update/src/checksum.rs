//! `SHA256SUMS.txt` (the output of `sha256sum`) and file hashing.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// Parses `sha256sum` output: `<64 hex>  <name>` or `<64 hex> *<name>`
/// (binary mode). Blank lines, comments and malformed lines are ignored.
/// Returns file name → lower-case hex digest.
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else { continue };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let name = rest.trim_start_matches([' ', '\t']);
        let name = name.strip_prefix('*').unwrap_or(name);
        // `sha256sum dir/*` writes paths; only the file name matters.
        let name = name.rsplit('/').next().unwrap_or(name);
        if name.is_empty() {
            continue;
        }
        out.insert(name.to_string(), hash.to_ascii_lowercase());
    }
    out
}

/// SHA-256 of a reader as lower-case hex.
pub fn sha256_reader(mut r: impl Read) -> std::io::Result<String> {
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    sha256_reader(std::fs::File::open(path)?)
}

pub fn sha256_bytes(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_real_release_file() {
        let s = parse_sums(include_str!("../tests/fixtures/SHA256SUMS-0.0.1.txt"));
        assert_eq!(s.len(), 9);
        assert_eq!(s["OpenDecks-Session-0.0.1-macos-arm64.dmg"], "ac054b8c10dfcdb2a73f424422c63048e8830209544f84aeac969714b6f9cc21");
        assert_eq!(s["opendecks-session_0.0.1_amd64.deb"], "cb14f0e6fc6a5e6eced1bf8bd5ce2e0e1817eb0624c9065779b228858ca1395d");
        assert!(s.contains_key("OpenDecks-Session-0.0.1-x86_64.AppImage"));
        assert!(!s.contains_key("SHA256SUMS.txt"));
    }

    #[test]
    fn tolerates_variants_and_junk() {
        let h = "AC054B8C10DFCDB2A73F424422C63048E8830209544F84AEAC969714B6F9CC21";
        let text = format!("# comment\n\n{h} *bin.dmg\r\n{h}  dist/sub/file.deb\nnot a line\n1234  short.txt\n{h}\n");
        let s = parse_sums(&text);
        assert_eq!(s.len(), 2);
        assert_eq!(s["bin.dmg"], h.to_ascii_lowercase());
        assert!(s.contains_key("file.deb"));
    }

    #[test]
    fn hashes_known_vectors() {
        assert_eq!(sha256_bytes(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_bytes(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_bytes(b"abc"));
    }
}
