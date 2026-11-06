//! Resumable downloads with progress and SHA-256 verification.
//!
//! The file grows as `<dest>.part`; an interrupted download resumes from
//! there (`Range`). Only a complete file whose checksum matches is renamed
//! to `<dest>`; a mismatch deletes the part file.

use crate::Error;
use crate::checksum::sha256_file;
use crate::http::Http;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Path of the partial file for `dest`.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Downloads `url` to `dest` and checks it against `sha256` (lower-case
/// hex). `size` (from the release) lets a complete part file skip the
/// request. `progress(done, total)` is called as bytes arrive.
pub fn download_verified(
    http: &dyn Http,
    url: &str,
    dest: &Path,
    size: Option<u64>,
    sha256: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<(), Error> {
    if dest.exists() && sha256_file(dest).is_ok_and(|h| h == sha256) {
        // Downloaded and verified before (e.g. an install that failed later).
        let n = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        progress(n, Some(n));
        return Ok(());
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = part_path(dest);
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if size.is_some_and(|s| have > s) {
        have = 0;
    }
    if size.is_none_or(|s| have < s) || have == 0 {
        fetch(http, url, &part, have, size, progress, cancel)?;
    }
    let got = sha256_file(&part)?;
    if got != sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(Error::Checksum { file: dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), expected: sha256.to_string(), got });
    }
    std::fs::rename(&part, dest)?;
    Ok(())
}

fn fetch(http: &dyn Http, url: &str, part: &Path, have: u64, size: Option<u64>, progress: &mut dyn FnMut(u64, Option<u64>), cancel: &AtomicBool) -> Result<(), Error> {
    let resp = http.get(url, (have > 0).then_some(have))?;
    let (append, mut done) = match resp.status {
        206 if have > 0 => (true, have),
        200 => (false, 0),
        // The part file is complete or the server lost track: start over.
        416 if have > 0 => {
            let _ = std::fs::remove_file(part);
            return fetch(http, url, part, 0, size, progress, cancel);
        }
        s => return Err(Error::Status(s, url.to_string())),
    };
    let total = size.or(resp.content_length.map(|n| n + done));
    let file = std::fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part)?;
    let mut file = std::io::BufWriter::new(file);
    let mut body = resp.body;
    let mut buf = vec![0u8; 1 << 16];
    progress(done, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            file.flush()?;
            return Err(Error::Cancelled);
        }
        let n = match body.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                // Keep what arrived: the next attempt resumes.
                file.flush()?;
                return Err(Error::Network(e.to_string()));
            }
        };
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    if let Some(t) = total.filter(|t| done < *t) {
        return Err(Error::Network(format!("connection ended after {done} of {t} bytes")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::sha256_bytes;
    use crate::http::mock::MockHttp;

    const URL: &str = "https://example.invalid/asset.dmg";

    fn payload() -> Vec<u8> {
        (0..300_000u32).map(|i| (i * 7 % 251) as u8).collect()
    }

    #[test]
    fn downloads_and_verifies() {
        let data = payload();
        let http = MockHttp::default().with(URL, data.clone());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        let mut seen = Vec::new();
        download_verified(&http, URL, &dest, Some(data.len() as u64), &sha256_bytes(&data), &mut |d, t| seen.push((d, t)), &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!part_path(&dest).exists());
        assert_eq!(seen.last().unwrap(), &(data.len() as u64, Some(data.len() as u64)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        // A second call finds the verified file and makes no request.
        let before = http.requests.lock().unwrap().len();
        download_verified(&http, URL, &dest, None, &sha256_bytes(&data), &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(http.requests.lock().unwrap().len(), before);
    }

    #[test]
    fn resumes_after_a_dropped_connection() {
        let data = payload();
        let http = MockHttp::default().with(URL, data.clone());
        http.cut.lock().unwrap().insert(URL.into(), 100_000);
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        let sum = sha256_bytes(&data);
        let size = Some(data.len() as u64);
        let e = download_verified(&http, URL, &dest, size, &sum, &mut |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(e, Error::Network(_)), "{e}");
        assert_eq!(std::fs::metadata(part_path(&dest)).unwrap().len(), 100_000);
        assert!(!dest.exists());
        download_verified(&http, URL, &dest, size, &sum, &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        let reqs = http.requests.lock().unwrap().clone();
        assert_eq!(reqs, vec![(URL.to_string(), None), (URL.to_string(), Some(100_000))]);
    }

    #[test]
    fn server_without_range_restarts_cleanly() {
        let data = payload();
        let mut http = MockHttp::default().with(URL, data.clone());
        http.no_range.push(URL.into());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        std::fs::write(part_path(&dest), &data[..5000]).unwrap();
        download_verified(&http, URL, &dest, Some(data.len() as u64), &sha256_bytes(&data), &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn complete_part_file_is_verified_without_a_request() {
        let data = payload();
        let http = MockHttp::default();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        std::fs::write(part_path(&dest), &data).unwrap();
        download_verified(&http, URL, &dest, Some(data.len() as u64), &sha256_bytes(&data), &mut |_, _| {}, &AtomicBool::new(false)).unwrap();
        assert!(http.requests.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn checksum_mismatch_deletes_the_file() {
        let data = payload();
        let http = MockHttp::default().with(URL, data.clone());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        let e = download_verified(&http, URL, &dest, None, &sha256_bytes(b"other"), &mut |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(e, Error::Checksum { .. }), "{e}");
        assert!(!dest.exists() && !part_path(&dest).exists());
    }

    #[test]
    fn cancel_keeps_the_part_file() {
        let data = payload();
        let http = MockHttp::default().with(URL, data.clone());
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a.dmg");
        let e = download_verified(&http, URL, &dest, None, &sha256_bytes(&data), &mut |_, _| {}, &AtomicBool::new(true)).unwrap_err();
        assert!(matches!(e, Error::Cancelled));
        assert!(part_path(&dest).exists() && !dest.exists());
    }

    #[test]
    fn http_errors_are_reported() {
        let http = MockHttp::default();
        let dir = tempfile::tempdir().unwrap();
        let e = download_verified(&http, URL, &dir.path().join("x"), None, "00", &mut |_, _| {}, &AtomicBool::new(false)).unwrap_err();
        assert!(matches!(e, Error::Status(404, _)));
    }
}
