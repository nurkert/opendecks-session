//! Discovery and ejection of mounted removable volumes.
//!
//! Polling based and dependency free: on macOS mounts under `/Volumes`, on
//! Linux the mount table filtered to removable/USB block devices.

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub mount_point: PathBuf,
    pub name: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Block device (Linux) or BSD name (macOS), used for ejecting.
    pub device_node: Option<String>,
}

/// Total and free bytes of the file system containing `path`.
pub fn space(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` a valid out pointer.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let frsize = st.f_frsize as u64;
    Some((st.f_blocks as u64 * frsize, st.f_bavail as u64 * frsize))
}

/// Mounted removable volumes.
pub fn list() -> Vec<Volume> {
    if let Some(dir) = std::env::var_os("OPENDECKS_VOLUMES_DIR") {
        // Test hook: every directory below is treated as a mounted stick.
        return list_dir_as_volumes(Path::new(&dir));
    }
    if cfg!(target_os = "macos") {
        list_macos()
    } else {
        list_linux()
    }
}

fn list_dir_as_volumes(dir: &Path) -> Vec<Volume> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<Volume> = read
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let p = e.path();
            let (total, free) = space(&p).unwrap_or((0, 0));
            Volume {
                name: e.file_name().to_string_lossy().into_owned(),
                mount_point: p,
                fs_type: "test".into(),
                total_bytes: total,
                free_bytes: free,
                device_node: None,
            }
        })
        .collect();
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out
}

fn list_macos() -> Vec<Volume> {
    let Ok(read) = std::fs::read_dir("/Volumes") else {
        return vec![];
    };
    let mut out = Vec::new();
    for e in read.flatten() {
        let p = e.path();
        // The boot volume appears as a symlink to "/".
        if std::fs::symlink_metadata(&p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        let Some(info) = macos_disk_info(&p) else {
            continue;
        };
        if !info.external {
            continue;
        }
        let (total, free) = space(&p).unwrap_or((0, 0));
        out.push(Volume {
            name: e.file_name().to_string_lossy().into_owned(),
            mount_point: p,
            fs_type: info.fs_type,
            total_bytes: total,
            free_bytes: free,
            device_node: info.device,
        });
    }
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out
}

struct MacDiskInfo {
    external: bool,
    fs_type: String,
    device: Option<String>,
}

fn macos_disk_info(mount: &Path) -> Option<MacDiskInfo> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (bool, String, Option<String>)>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((external, fs, dev)) = cache.lock().ok()?.get(mount).cloned() {
        return Some(MacDiskInfo {
            external,
            fs_type: fs,
            device: dev,
        });
    }
    let out = Command::new("diskutil")
        .arg("info")
        .arg(mount)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |name: &str| {
        text.lines().find_map(|l| {
            l.trim()
                .strip_prefix(name)
                .map(|v| v.trim().trim_start_matches(':').trim().to_string())
        })
    };
    let location = field("Device Location").unwrap_or_default();
    let removable = field("Removable Media").unwrap_or_default();
    let protocol = field("Protocol").unwrap_or_default();
    let read_only = field("Volume Read-Only")
        .or_else(|| field("Media Read-Only"))
        .unwrap_or_default();
    // Disk images, network volumes and read-only media are never sticks.
    let excluded = protocol.eq_ignore_ascii_case("disk image")
        || protocol.eq_ignore_ascii_case("virtual interface")
        || read_only.to_ascii_lowercase().starts_with("yes")
        || mount.join("Backups.backupdb").exists()
        || mount.join(".timemachine").exists();
    let external = !excluded
        && (location.eq_ignore_ascii_case("external")
            || removable.eq_ignore_ascii_case("removable")
            || protocol.eq_ignore_ascii_case("usb")
            || protocol.eq_ignore_ascii_case("secure digital"));
    let fs_type = field("Type (Bundle)").unwrap_or_default();
    let device = field("Device Identifier");
    cache.lock().ok()?.insert(
        mount.to_path_buf(),
        (external, fs_type.clone(), device.clone()),
    );
    Some(MacDiskInfo {
        external,
        fs_type,
        device,
    })
}

/// Unescapes the octal escapes used in /proc/mounts (`\040` = space).
fn unescape_mount(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 4], 8) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn linux_is_removable(device: &str) -> bool {
    let name = Path::new(device)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let sys = Path::new("/sys/class/block").join(&name);
    let Ok(real) = std::fs::canonicalize(&sys) else {
        return false;
    };
    if real.to_string_lossy().contains("/usb") {
        return true;
    }
    // Partition: check the parent disk's removable flag.
    let disk = real.parent().map(|p| p.join("removable"));
    let own = real.join("removable");
    [Some(own), disk]
        .into_iter()
        .flatten()
        .any(|p| std::fs::read_to_string(p).is_ok_and(|v| v.trim() == "1"))
}

fn list_linux() -> Vec<Volume> {
    let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") else {
        return vec![];
    };
    let mut out = Vec::new();
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let (Some(dev), Some(mp), Some(fs)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !dev.starts_with("/dev/") {
            continue;
        }
        let mp = unescape_mount(mp);
        if !(mp.starts_with("/media/") || mp.starts_with("/run/media/") || mp.starts_with("/mnt/"))
        {
            continue;
        }
        if !linux_is_removable(dev) {
            continue;
        }
        let p = PathBuf::from(&mp);
        let (total, free) = space(&p).unwrap_or((0, 0));
        out.push(Volume {
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| mp.clone()),
            mount_point: p,
            fs_type: fs.to_string(),
            total_bytes: total,
            free_bytes: free,
            device_node: Some(dev.to_string()),
        });
    }
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out
}

#[derive(Debug, thiserror::Error)]
#[error("eject failed: {0}")]
pub struct EjectError(pub String);

/// Unmounts and ejects a volume.
pub fn eject(volume: &Volume) -> Result<(), EjectError> {
    if std::env::var_os("OPENDECKS_VOLUMES_DIR").is_some() {
        return Ok(());
    }
    let run = |cmd: &mut Command| -> Result<(), EjectError> {
        let out = cmd.output().map_err(|e| EjectError(e.to_string()))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(EjectError(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    };
    if cfg!(target_os = "macos") {
        run(Command::new("diskutil")
            .arg("eject")
            .arg(&volume.mount_point))
    } else if let Some(dev) = &volume.device_node {
        run(Command::new("udisksctl").args(["unmount", "-b", dev]))
            .or_else(|_| run(Command::new("umount").arg(&volume.mount_point)))?;
        // Powering off is best effort (not all devices support it).
        let _ = run(Command::new("udisksctl").args(["power-off", "-b", dev]));
        Ok(())
    } else {
        run(Command::new("umount").arg(&volume.mount_point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_mount_points() {
        assert_eq!(
            unescape_mount("/media/me/MY\\040STICK"),
            "/media/me/MY STICK"
        );
    }

    #[test]
    fn space_of_tempdir() {
        let d = tempfile::tempdir().unwrap();
        let (total, free) = space(d.path()).unwrap();
        assert!(total > 0 && free <= total);
    }

    #[test]
    fn test_hook_lists_directories() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("STICK")).unwrap();
        let v = list_dir_as_volumes(d.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "STICK");
    }
}
