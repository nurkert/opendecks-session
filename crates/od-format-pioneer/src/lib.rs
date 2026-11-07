//! Reader and writer for the legacy Pioneer DJ USB export format.
//!
//! This crate is written for interoperability only (see `docs/SPEC.md` §2 and
//! §8.5). It is a clean implementation based on public documentation:
//!
//! * Deep Symmetry, "rekordbox Export Structure Analysis"
//!   (<https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/>) and
//!   the Kaitai Struct definitions `rekordbox_pdb.ksy` / `rekordbox_anlz.ksy`
//!   in <https://github.com/Deep-Symmetry/crate-digger>.
//! * Field notes published with the `rekordcrate` project
//!   (<https://github.com/holzhaus/rekordcrate>) and the `baken-export`
//!   project (<https://crates.io/crates/baken-export>), in particular
//!   observations about row padding, page header counters and the
//!   `USBANLZ` directory hash.
//!
//! No code from those projects is included. The crate is pure: bytes in,
//! bytes out. Placing files on a stick is the job of the sync engine.
//!
//! * [`pdb`] reads and writes `/PIONEER/rekordbox/export.pdb`.
//! * [`anlz`] reads and writes `ANLZ0000.DAT`, `.EXT` and `.2EX`.
//! * [`mysetting`] writes `/PIONEER/MYSETTING.DAT` (player preferences).

pub mod anlz;
pub mod mysetting;
pub mod pdb;

mod bytes;
mod palette;

pub use palette::{HOT_CUE_PALETTE, TRACK_COLORS};

/// Errors while parsing Pioneer files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unexpected end of data at offset {0:#x}")]
    Truncated(usize),
    #[error("bad magic: expected {expected}, found {found:?}")]
    BadMagic {
        expected: &'static str,
        found: Vec<u8>,
    },
    #[error("invalid data: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;
