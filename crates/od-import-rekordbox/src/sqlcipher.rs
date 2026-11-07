//! Read-only decryption of SQLCipher 4 databases into plain SQLite files.
//!
//! rekordbox 6/7 keep their library in `master.db`, an SQLCipher 4 database
//! with default settings. Instead of linking an SQLCipher build of SQLite
//! (Cargo would unify it with the plain SQLite the rest of the workspace
//! uses), the pages are decrypted here and written to a temporary plain
//! SQLite file, which is then opened read-only with ordinary SQLite.
//!
//! SQLCipher 4 defaults (<https://www.zetetic.net/sqlcipher/sqlcipher-api/>,
//! "cipher_compatibility = 4"):
//!
//! * key derivation: PBKDF2-HMAC-SHA512, 256 000 iterations, 32 byte key,
//!   salt = the first 16 bytes of the file, password = the passphrase bytes;
//! * HMAC key: PBKDF2-HMAC-SHA512 over the key with the salt XOR 0x3a,
//!   2 iterations;
//! * every page: AES-256-CBC encrypted payload, followed by a reserved area
//!   of 80 bytes = 16 byte IV + 64 byte HMAC-SHA512 over
//!   `payload || IV || page number (u32 little endian)`;
//! * page 1 starts with the salt instead of the "SQLite format 3" magic; its
//!   payload starts at offset 16.
//!
//! WAL files keep their frame headers in clear text; the page images inside
//! the frames are encrypted the same way as in the main file.

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::NoPadding};
use hmac::{Hmac, Mac};
use sha2::Sha512;
use std::path::Path;

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type HmacSha512 = Hmac<Sha512>;

const SALT_LEN: usize = 16;
const IV_LEN: usize = 16;
const HMAC_LEN: usize = 64;
/// IV + HMAC, already a multiple of the AES block size.
const RESERVE: usize = IV_LEN + HMAC_LEN;
const KDF_ITER: u32 = 256_000;
const HMAC_SALT_MASK: u8 = 0x3a;
const FAST_KDF_ITER: u32 = 2;
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

#[derive(Debug, thiserror::Error)]
pub enum CipherError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("file is too short to be a database")]
    TooShort,
    #[error("cannot decrypt the database (wrong key or not an SQLCipher 4 file)")]
    BadKey,
    #[error("page {0} failed its integrity check")]
    BadPage(u32),
}

/// Keys derived from a passphrase and a file's salt.
#[derive(Clone)]
pub struct Keys {
    enc: [u8; 32],
    mac: [u8; 32],
}

impl Keys {
    pub fn derive(passphrase: &[u8], salt: &[u8]) -> Self {
        let mut enc = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha512>(passphrase, salt, KDF_ITER, &mut enc);
        let mac_salt: Vec<u8> = salt.iter().map(|b| b ^ HMAC_SALT_MASK).collect();
        let mut mac = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha512>(&enc, &mac_salt, FAST_KDF_ITER, &mut mac);
        Self { enc, mac }
    }
}

fn page_mac(keys: &Keys, payload: &[u8], iv: &[u8], pgno: u32) -> HmacSha512 {
    let mut m = <HmacSha512 as Mac>::new_from_slice(&keys.mac).expect("any key length");
    m.update(payload);
    m.update(iv);
    m.update(&pgno.to_le_bytes());
    m
}

/// Decrypts one page image in place layout: returns the plain page (same
/// size, reserved area zeroed, page 1 with the SQLite magic restored).
fn decrypt_page(keys: &Keys, page: &[u8], pgno: u32) -> Result<Vec<u8>, CipherError> {
    let size = page.len();
    let start = if pgno == 1 { SALT_LEN } else { 0 };
    let payload = &page[start..size - RESERVE];
    let iv = &page[size - RESERVE..size - HMAC_LEN];
    let tag = &page[size - HMAC_LEN..];
    page_mac(keys, payload, iv, pgno)
        .verify_slice(tag)
        .map_err(|_| CipherError::BadPage(pgno))?;
    let mut out = vec![0u8; size];
    out[start..size - RESERVE].copy_from_slice(payload);
    Aes256CbcDec::new(&keys.enc.into(), iv.into())
        .decrypt_padded_mut::<NoPadding>(&mut out[start..size - RESERVE])
        .map_err(|_| CipherError::BadPage(pgno))?;
    if pgno == 1 {
        out[..SALT_LEN].copy_from_slice(SQLITE_MAGIC);
    }
    Ok(out)
}

/// Whether `bytes` start like a plain (unencrypted) SQLite database.
pub fn is_plain_sqlite(bytes: &[u8]) -> bool {
    bytes.len() >= 16 && &bytes[..16] == SQLITE_MAGIC
}

/// Decrypts the database `src` (plus `src-wal` if present) with
/// `passphrase` and writes a plain SQLite file to `dest`. The result is in
/// rollback-journal mode so that it can be opened read-only without side
/// files.
pub fn decrypt_file(src: &Path, dest: &Path, passphrase: &[u8]) -> Result<(), CipherError> {
    let data = std::fs::read(src)?;
    let wal_path = {
        let mut p = src.as_os_str().to_owned();
        p.push("-wal");
        std::path::PathBuf::from(p)
    };
    let wal = std::fs::read(&wal_path).ok();
    let plain = decrypt_bytes(&data, wal.as_deref(), passphrase)?;
    std::fs::write(dest, plain)?;
    Ok(())
}

/// Decrypts database bytes (and optional WAL bytes) into a plain database.
pub fn decrypt_bytes(
    data: &[u8],
    wal: Option<&[u8]>,
    passphrase: &[u8],
) -> Result<Vec<u8>, CipherError> {
    if data.len() < 512 {
        return Err(CipherError::TooShort);
    }
    let keys = Keys::derive(passphrase, &data[..SALT_LEN]);
    // The page size is inside the encrypted header; find it by checking the
    // HMAC of page 1 for each legal size (4096 is the SQLCipher default).
    let page_size = [4096usize, 1024, 2048, 8192, 16384, 32768, 65536]
        .into_iter()
        .find(|&ps| data.len() >= ps && decrypt_page(&keys, &data[..ps], 1).is_ok())
        .ok_or(CipherError::BadKey)?;
    let mut pages: Vec<Vec<u8>> = Vec::with_capacity(data.len() / page_size);
    for (i, chunk) in data.chunks(page_size).enumerate() {
        if chunk.len() < page_size {
            break; // trailing garbage, ignore
        }
        pages.push(decrypt_page(&keys, chunk, i as u32 + 1)?);
    }
    if let Some(wal) = wal {
        apply_wal(&keys, wal, page_size, &mut pages)?;
    }
    if let Some(p1) = pages.first_mut() {
        // File format read/write versions: 1 = rollback journal.
        p1[18] = 1;
        p1[19] = 1;
    }
    Ok(pages.concat())
}

/// SQLite's WAL checksum (<https://www.sqlite.org/fileformat2.html#walformat>).
fn wal_checksum(big_endian: bool, data: &[u8], mut s0: u32, mut s1: u32) -> (u32, u32) {
    for w in data.chunks_exact(8) {
        let (a, b) = if big_endian {
            (
                u32::from_be_bytes(w[0..4].try_into().unwrap()),
                u32::from_be_bytes(w[4..8].try_into().unwrap()),
            )
        } else {
            (
                u32::from_le_bytes(w[0..4].try_into().unwrap()),
                u32::from_le_bytes(w[4..8].try_into().unwrap()),
            )
        };
        s0 = s0.wrapping_add(a).wrapping_add(s1);
        s1 = s1.wrapping_add(b).wrapping_add(s0);
    }
    (s0, s1)
}

/// Applies all committed, checksum-valid frames of a WAL file.
fn apply_wal(
    keys: &Keys,
    wal: &[u8],
    page_size: usize,
    pages: &mut Vec<Vec<u8>>,
) -> Result<(), CipherError> {
    const HDR: usize = 32;
    const FHDR: usize = 24;
    if wal.len() < HDR {
        return Ok(());
    }
    let be32 = |b: &[u8], at: usize| u32::from_be_bytes(b[at..at + 4].try_into().unwrap());
    let magic = be32(wal, 0);
    if magic & !1 != 0x377f_0682 {
        return Ok(()); // not a WAL file (or empty): nothing to apply
    }
    let big_endian = magic & 1 == 1;
    if be32(wal, 8) as usize != page_size {
        return Ok(());
    }
    let (salt1, salt2) = (be32(wal, 16), be32(wal, 20));
    let (mut s0, mut s1) = wal_checksum(big_endian, &wal[..24], 0, 0);
    if (s0, s1) != (be32(wal, 24), be32(wal, 28)) {
        return Ok(());
    }
    let mut pending: Vec<(u32, &[u8])> = Vec::new();
    let mut committed: Vec<(u32, &[u8])> = Vec::new();
    let mut db_pages: Option<u32> = None;
    let mut at = HDR;
    while at + FHDR + page_size <= wal.len() {
        let fh = &wal[at..at + FHDR];
        let body = &wal[at + FHDR..at + FHDR + page_size];
        if be32(fh, 8) != salt1 || be32(fh, 12) != salt2 {
            break;
        }
        (s0, s1) = wal_checksum(big_endian, &fh[..8], s0, s1);
        (s0, s1) = wal_checksum(big_endian, body, s0, s1);
        if (s0, s1) != (be32(fh, 16), be32(fh, 20)) {
            break;
        }
        pending.push((be32(fh, 0), body));
        let commit = be32(fh, 4);
        if commit != 0 {
            committed.append(&mut pending);
            db_pages = Some(commit);
        }
        at += FHDR + page_size;
    }
    for (pgno, body) in committed {
        if pgno == 0 {
            continue;
        }
        let plain = decrypt_page(keys, body, pgno)?;
        let idx = pgno as usize - 1;
        if pages.len() <= idx {
            pages.resize(idx + 1, vec![0u8; page_size]);
        }
        pages[idx] = plain;
    }
    if let Some(n) = db_pages {
        pages.truncate(n as usize);
        // Keep the in-header page count consistent with the file size.
        if let Some(p1) = pages.first_mut() {
            p1[28..32].copy_from_slice(&n.to_be_bytes());
        }
    }
    Ok(())
}

/// Test helper: encrypts a plain SQLite database the way SQLCipher 4 does,
/// so the decryption path can be tested without an SQLCipher build.
#[cfg(test)]
pub(crate) fn encrypt_bytes(
    plain: &[u8],
    passphrase: &[u8],
    salt: [u8; 16],
    page_size: usize,
) -> Vec<u8> {
    use aes::cipher::BlockEncryptMut;
    type Enc = cbc::Encryptor<aes::Aes256>;
    assert!(is_plain_sqlite(plain));
    assert_eq!(
        plain[20] as usize, RESERVE,
        "plain db must reserve 80 bytes per page"
    );
    let keys = Keys::derive(passphrase, &salt);
    let mut out = Vec::with_capacity(plain.len());
    for (i, page) in plain.chunks(page_size).enumerate() {
        let pgno = i as u32 + 1;
        let start = if pgno == 1 { SALT_LEN } else { 0 };
        let mut buf = page[..page_size - RESERVE].to_vec();
        let iv: [u8; 16] =
            std::array::from_fn(|k| (pgno as u8).wrapping_mul(31).wrapping_add(k as u8));
        Enc::new(&keys.enc.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf[start..], page_size - RESERVE - start)
            .unwrap();
        if pgno == 1 {
            buf[..SALT_LEN].copy_from_slice(&salt);
        }
        let tag = page_mac(&keys, &buf[start..], &iv, pgno)
            .finalize()
            .into_bytes();
        out.extend_from_slice(&buf);
        out.extend_from_slice(&iv);
        out.extend_from_slice(&tag);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wal_checksum_is_cumulative() {
        let data: Vec<u8> = (0..32u8).collect();
        let (a, b) = wal_checksum(true, &data, 0, 0);
        let (c, d) = wal_checksum(true, &data[..16], 0, 0);
        assert_eq!(wal_checksum(true, &data[16..], c, d), (a, b));
    }
}
