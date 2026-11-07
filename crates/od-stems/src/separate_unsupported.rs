//! Separation API for targets without ONNX Runtime (Intel Macs): the same
//! types and functions, every separation attempt reports
//! [`Error::Unsupported`]. Cached stems keep working.

use crate::{Error, Stems};
use od_decode::DecodedAudio;
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub const MODEL_RATE: u32 = 44_100;
pub const SEGMENT: usize = 343_980;
pub const OVERLAP: f32 = 0.25;

/// Options for [`Separator::new`].
#[derive(Clone, Debug)]
pub struct SeparatorOptions {
    pub threads: usize,
    pub overlap: f32,
}

impl Default for SeparatorOptions {
    fn default() -> Self {
        Self {
            threads: 0,
            overlap: OVERLAP,
        }
    }
}

/// Never constructed on this platform.
pub struct Separator {
    _private: (),
}

impl Separator {
    pub fn new(_model_path: &Path, _opts: &SeparatorOptions) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }

    pub fn load_default() -> Result<Self, Error> {
        Err(Error::Unsupported)
    }

    pub fn separate_file(
        &mut self,
        _path: &Path,
        _progress: &mut dyn FnMut(f32),
        _cancel: &AtomicBool,
    ) -> Result<Stems, Error> {
        Err(Error::Unsupported)
    }

    pub fn separate(
        &mut self,
        _audio: &DecodedAudio,
        _progress: &mut dyn FnMut(f32),
        _cancel: &AtomicBool,
    ) -> Result<Stems, Error> {
        Err(Error::Unsupported)
    }
}

pub fn separate(
    _path: &Path,
    _progress: &mut dyn FnMut(f32),
    _cancel: &AtomicBool,
) -> Result<Stems, Error> {
    Err(Error::Unsupported)
}
