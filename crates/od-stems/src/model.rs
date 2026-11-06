//! The separation model: identity, location and first-use download.

use std::path::PathBuf;
#[cfg(feature = "separation")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "separation")]
use crate::Error;

/// A downloadable model file.
#[derive(Clone, Copy, Debug)]
pub struct ModelInfo {
    /// File name under `data_dir()/models/`.
    pub file_name: &'static str,
    pub url: &'static str,
    /// Lower-case hex SHA-256 of the file.
    pub sha256: &'static str,
    pub size: u64,
    pub license: &'static str,
}

/// HT-Demucs v4 (`htdemucs`, 4 sources), ONNX export with fp16-stored
/// weights, from <https://huggingface.co/StemSplitio/htdemucs-onnx>
/// (revision `d54ed9eb60e258ea82131c6ee14578628816456a`). Weights and code
/// © Meta Platforms, Inc., MIT License (facebookresearch/demucs); ONNX
/// export by StemSplit, MIT License.
pub const MODEL: ModelInfo = ModelInfo {
    file_name: "htdemucs_fp16weights.onnx",
    url: "https://huggingface.co/StemSplitio/htdemucs-onnx/resolve/d54ed9eb60e258ea82131c6ee14578628816456a/htdemucs_fp16weights.onnx",
    sha256: "d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a",
    size: 165_612_636,
    license: "MIT (Demucs © Meta Platforms, Inc.; ONNX export © StemSplit)",
};

/// `od_library::data_dir()/models`.
pub fn models_dir() -> PathBuf {
    od_library::data_dir().join("models")
}

/// Where the model lives once fetched.
pub fn model_path() -> PathBuf {
    models_dir().join(MODEL.file_name)
}

/// Whether the model file is present with the expected size (the hash is
/// verified at download time only).
pub fn model_available() -> bool {
    std::fs::metadata(model_path()).is_ok_and(|m| m.len() == MODEL.size)
}

/// Downloads the model into [`models_dir`] unless already present, verifying
/// its SHA-256. `progress` receives (bytes done, bytes total). Interrupted
/// or cancelled downloads leave `<file>.part` and resume on the next call.
/// Alternatively place the file there manually ([`MODEL`] has URL and hash).
#[cfg(feature = "separation")]
pub fn fetch_model(progress: &mut dyn FnMut(u64, u64), cancel: &AtomicBool) -> Result<PathBuf, Error> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};

    let path = model_path();
    if model_available() {
        return Ok(path);
    }
    std::fs::create_dir_all(models_dir())?;
    let tmp = path.with_extension("onnx.part");
    let net = |e: &dyn std::fmt::Display| {
        Error::Download(format!(
            "{e} — check the network/firewall, or download {} manually (SHA-256 {}) to {}",
            MODEL.url,
            MODEL.sha256,
            path.display()
        ))
    };
    // Resume a previous partial download: hash what is already there.
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    if let Ok(mut f) = std::fs::File::open(&tmp) {
        let len = f.metadata()?.len();
        if len < MODEL.size {
            let mut buf = vec![0u8; 1 << 20];
            loop {
                let n = f.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
                done += n as u64;
            }
        }
    }
    if done == 0 || done >= MODEL.size {
        hasher = Sha256::new();
        done = 0;
    }
    // Connect timeout: ureq falls through to the next address (e.g. a broken
    // IPv6 route) instead of blocking in connect(); no unbounded waits.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(3600)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent.get(MODEL.url);
    if done > 0 {
        req = req.header("Range", format!("bytes={done}-"));
    }
    let mut resp = req.call().map_err(|e| net(&e))?;
    let status = resp.status().as_u16();
    let append = match status {
        206 if done > 0 => true,
        200 => {
            hasher = Sha256::new();
            done = 0;
            false
        }
        s => return Err(net(&format!("HTTP status {s}"))),
    };
    let mut reader = resp.body_mut().as_reader();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&tmp)?;
    let mut file = std::io::BufWriter::new(file);
    let mut buf = vec![0u8; 1 << 16];
    progress(done, MODEL.size);
    loop {
        if cancel.load(Ordering::Relaxed) {
            // Keep the .part file: the next call resumes.
            file.flush()?;
            return Err(Error::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                file.flush()?;
                return Err(net(&e));
            }
        };
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, MODEL.size);
    }
    file.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done < MODEL.size {
        return Err(net(&format!("connection ended after {done} of {} bytes (will resume)", MODEL.size)));
    }
    if got != MODEL.sha256 || done != MODEL.size {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Download(format!(
            "checksum mismatch: got {got} ({done} bytes), expected {} ({} bytes)",
            MODEL.sha256, MODEL.size
        )));
    }
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}
