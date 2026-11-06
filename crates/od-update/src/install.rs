//! Installing a verified download.
//!
//! The decision ([`strategy`]) and the helper script ([`HELPER_SCRIPT`]) are
//! pure and tested; the steps that touch the system (`hdiutil`, `ditto`,
//! `pkexec`, spawning the helper) are thin wrappers around them.
//!
//! Nothing replaces the running app while it runs: the new version is staged
//! next to the old one, and a detached helper swaps them after the app has
//! quit, then starts the new version. Any failure before the swap leaves the
//! current version untouched; a failed swap renames the old version back.

use crate::Error;
use crate::platform::InstallKind;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What installing means on this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Copy the new `.app` from the DMG next to the bundle, swap after quit.
    SwapMacBundle { bundle: PathBuf },
    /// The bundle's folder is not writable (or the app runs translocated /
    /// from a disk image): download and verify the DMG, then open it.
    OpenDmg,
    /// Download the new AppImage next to the old one, swap after quit.
    SwapAppImage { path: PathBuf },
    /// Hand the `.deb` to the system installer through pkexec.
    InstallDeb { tool: DebTool },
    /// Nothing we can do in place: open the release page.
    OpenReleasePage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebTool {
    AptGet,
    Dpkg,
}

/// Facts about the system the strategy depends on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// The folder holding the bundle / AppImage accepts new files.
    pub dir_writable: bool,
    pub pkexec: bool,
    pub apt_get: bool,
    pub dpkg: bool,
}

impl Probe {
    /// Looks at the real system for `kind`.
    pub fn of(kind: &InstallKind) -> Self {
        let dir = match kind {
            InstallKind::MacBundle { bundle } => bundle.parent().filter(|_| !translocated(bundle)),
            InstallKind::AppImage { path } => path.parent(),
            _ => None,
        };
        Probe {
            dir_writable: dir.is_some_and(dir_writable),
            pkexec: matches!(kind, InstallKind::Deb) && on_path("pkexec").is_some(),
            apt_get: matches!(kind, InstallKind::Deb) && on_path("apt-get").is_some(),
            dpkg: matches!(kind, InstallKind::Deb) && on_path("dpkg").is_some(),
        }
    }
}

/// Picks the install strategy. `has_asset`: the release has a file for this
/// install kind.
pub fn strategy(kind: &InstallKind, has_asset: bool, probe: Probe) -> Strategy {
    if !has_asset {
        return Strategy::OpenReleasePage;
    }
    match kind {
        InstallKind::MacBundle { bundle } if probe.dir_writable => Strategy::SwapMacBundle { bundle: bundle.clone() },
        InstallKind::MacBundle { .. } => Strategy::OpenDmg,
        InstallKind::AppImage { path } if probe.dir_writable => Strategy::SwapAppImage { path: path.clone() },
        InstallKind::AppImage { .. } => Strategy::OpenReleasePage,
        InstallKind::Deb if probe.pkexec && probe.apt_get => Strategy::InstallDeb { tool: DebTool::AptGet },
        InstallKind::Deb if probe.pkexec && probe.dpkg => Strategy::InstallDeb { tool: DebTool::Dpkg },
        InstallKind::Deb | InstallKind::Unknown => Strategy::OpenReleasePage,
    }
}

/// macOS App Translocation or a mounted disk image: the bundle cannot be
/// replaced where it is.
pub fn translocated(bundle: &Path) -> bool {
    let s = bundle.to_string_lossy();
    s.contains("/AppTranslocation/") || s.starts_with("/Volumes/")
}

/// True when a file can be created in `dir`.
pub fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".opendecks-write-test-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// An executable on `$PATH`.
pub fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(name)).find(|p| p.is_file())
}

/// Where the new version is staged before the swap: a hidden sibling of
/// the installed bundle / AppImage, so the swap is a rename on one volume.
pub fn staging_path(current: &Path, version: &str) -> PathBuf {
    let name = current.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    current.with_file_name(format!(".{name}.update-{version}"))
}

/// The detached helper. Arguments: `MODE PID CURRENT NEW CMD...`.
///
/// - waits (up to 2 minutes) until process `PID` has exited,
/// - `swap`: replaces `CURRENT` with `NEW` — a file by one atomic rename; a
///   directory (app bundle) by renaming the old one aside, the new one in
///   and deleting the old one, renaming it back if the second step fails;
///   then removes the quarantine attribute from the new bundle only,
/// - `wait`: changes nothing (the package manager already installed),
/// - finally starts `CMD...` (the new version, or the old one after a
///   failure) and exits with 0 on success.
pub const HELPER_SCRIPT: &str = r#"#!/bin/sh
# OpenDecks Session update helper (written by the app, see crates/od-update).
mode="$1"; pid="$2"; cur="$3"; new="$4"; shift 4
i=0
while kill -0 "$pid" 2>/dev/null; do
  i=$((i + 1))
  [ "$i" -gt 600 ] && exit 2
  sleep 0.2
done
status=0
if [ "$mode" = swap ]; then
  if [ ! -e "$new" ]; then
    status=1
  elif [ -d "$cur" ]; then
    old="$cur.old-update"
    rm -rf "$old"
    if mv "$cur" "$old"; then
      if mv "$new" "$cur"; then
        rm -rf "$old"
        if command -v xattr >/dev/null 2>&1; then xattr -dr com.apple.quarantine "$cur" 2>/dev/null; fi
      else
        mv "$old" "$cur"
        status=1
      fi
    else
      status=1
    fi
  else
    chmod +x "$new" 2>/dev/null
    mv -f "$new" "$cur" || status=1
  fi
fi
[ "$status" -ne 0 ] && rm -rf "$new"
[ "$#" -gt 0 ] && "$@" >/dev/null 2>&1 &
exit "$status"
"#;

/// How the update is completed once the app quits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finish {
    pub mode: &'static str,
    pub current: PathBuf,
    pub new: PathBuf,
    /// Starts the app again.
    pub relaunch: Vec<OsString>,
}

impl Finish {
    /// Writes the helper to `dir` and starts it detached; it waits for this
    /// process to exit. The caller quits right after.
    pub fn spawn(&self, dir: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(dir)?;
        let script = dir.join("update-helper.sh");
        std::fs::write(&script, HELPER_SCRIPT)?;
        let mut cmd = Command::new("/bin/sh");
        cmd.arg(&script).arg(self.mode).arg(std::process::id().to_string()).arg(&self.current).arg(&self.new).args(&self.relaunch);
        detach(&mut cmd);
        cmd.spawn().map_err(|e| Error::Install(format!("could not start the update helper: {e}")))?;
        Ok(())
    }
}

/// No terminal, own process group: survives the app quitting.
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
}

/// Opens a file or URL with the desktop (`open` / `xdg-open`).
pub fn open(target: &std::ffi::OsStr) -> Result<(), Error> {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut cmd = Command::new(opener);
    cmd.arg(target);
    detach(&mut cmd);
    cmd.spawn().map(|_| ()).map_err(|e| Error::Install(format!("{opener}: {e}")))
}

fn run(cmd: &mut Command, what: &str) -> Result<String, Error> {
    let out = cmd.stdin(Stdio::null()).output().map_err(|e| Error::Install(format!("{what}: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(Error::Install(format!("{what} failed ({}): {}", out.status, err.trim())))
    }
}

/// macOS: copies the `.app` out of the verified `dmg` to `staging`
/// (mount read-only, `ditto`, check the signature, detach).
pub fn stage_from_dmg(dmg: &Path, mount_dir: &Path, staging: &Path) -> Result<(), Error> {
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(mount_dir)?;
    run(
        Command::new("hdiutil").args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-noverify", "-mountpoint"]).arg(mount_dir).arg(dmg),
        "hdiutil attach",
    )?;
    let result = (|| {
        let app = std::fs::read_dir(mount_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "app") && p.is_dir())
            .ok_or_else(|| Error::Install("the disk image holds no app".into()))?;
        run(Command::new("ditto").arg(&app).arg(staging), "ditto")?;
        run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(staging), "codesign --verify")?;
        Ok(())
    })();
    if run(Command::new("hdiutil").arg("detach").arg(mount_dir), "hdiutil detach").is_err() {
        let _ = run(Command::new("hdiutil").args(["detach", "-force"]).arg(mount_dir), "hdiutil detach -force");
    }
    let _ = std::fs::remove_dir(mount_dir);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(staging);
    }
    result
}

/// The command that installs `deb` with the system installer.
pub fn deb_command(tool: DebTool, deb: &Path) -> Vec<OsString> {
    let mut c: Vec<OsString> = vec!["pkexec".into()];
    match tool {
        DebTool::AptGet => c.extend(["apt-get".into(), "install".into(), "-y".into(), "--allow-downgrades".into()]),
        DebTool::Dpkg => c.extend(["dpkg".into(), "-i".into()]),
    }
    c.push(deb.as_os_str().to_owned());
    c
}

/// Installs the verified `.deb` (blocks while the user authenticates).
pub fn install_deb(tool: DebTool, deb: &Path) -> Result<(), Error> {
    let c = deb_command(tool, deb);
    let mut cmd = Command::new(&c[0]);
    cmd.args(&c[1..]);
    match run(&mut cmd, "pkexec") {
        Ok(_) => Ok(()),
        // pkexec: 126 = not authorised / dialog dismissed, 127 = no agent.
        Err(Error::Install(m)) if m.contains("126") || m.contains("127") => Err(Error::Install("authentication was cancelled or no authentication agent is running".into())),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn probe(dir_writable: bool, pkexec: bool, apt_get: bool, dpkg: bool) -> Probe {
        Probe { dir_writable, pkexec, apt_get, dpkg }
    }

    #[test]
    fn strategy_matrix() {
        let mac = InstallKind::MacBundle { bundle: p("/Applications/OpenDecks Session.app") };
        let app = InstallKind::AppImage { path: p("/home/me/OpenDecks.AppImage") };
        let all = probe(true, true, true, true);
        let none = Probe::default();
        assert_eq!(strategy(&mac, true, all), Strategy::SwapMacBundle { bundle: p("/Applications/OpenDecks Session.app") });
        assert_eq!(strategy(&mac, true, none), Strategy::OpenDmg);
        assert_eq!(strategy(&app, true, all), Strategy::SwapAppImage { path: p("/home/me/OpenDecks.AppImage") });
        assert_eq!(strategy(&app, true, none), Strategy::OpenReleasePage);
        assert_eq!(strategy(&InstallKind::Deb, true, all), Strategy::InstallDeb { tool: DebTool::AptGet });
        assert_eq!(strategy(&InstallKind::Deb, true, probe(false, true, false, true)), Strategy::InstallDeb { tool: DebTool::Dpkg });
        // No pkexec: the release page.
        assert_eq!(strategy(&InstallKind::Deb, true, probe(true, false, true, true)), Strategy::OpenReleasePage);
        assert_eq!(strategy(&InstallKind::Unknown, true, all), Strategy::OpenReleasePage);
        // A release without our asset: the release page, whatever the install.
        for k in [&mac, &app, &InstallKind::Deb] {
            assert_eq!(strategy(k, false, all), Strategy::OpenReleasePage);
        }
    }

    #[test]
    fn translocated_bundles_are_not_replaced() {
        assert!(translocated(&p("/private/var/folders/xy/T/AppTranslocation/1234/d/OpenDecks Session.app")));
        assert!(translocated(&p("/Volumes/OpenDecks Session/OpenDecks Session.app")));
        assert!(!translocated(&p("/Applications/OpenDecks Session.app")));
        let k = InstallKind::MacBundle { bundle: p("/Volumes/OpenDecks Session/OpenDecks Session.app") };
        assert!(!Probe::of(&k).dir_writable);
    }

    #[test]
    fn writable_dirs() {
        let d = tempfile::tempdir().unwrap();
        assert!(dir_writable(d.path()));
        assert!(!dir_writable(&d.path().join("missing")));
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0, "probe file left behind");
    }

    #[test]
    fn staging_is_a_hidden_sibling() {
        assert_eq!(staging_path(&p("/Applications/OpenDecks Session.app"), "0.2.0"), p("/Applications/.OpenDecks Session.app.update-0.2.0"));
        assert_eq!(staging_path(&p("/home/me/A.AppImage"), "1.0.0"), p("/home/me/.A.AppImage.update-1.0.0"));
    }

    #[test]
    fn deb_commands() {
        let c = deb_command(DebTool::AptGet, &p("/tmp/u/opendecks-session_0.2.0_amd64.deb"));
        assert_eq!(c, ["pkexec", "apt-get", "install", "-y", "--allow-downgrades", "/tmp/u/opendecks-session_0.2.0_amd64.deb"].map(OsString::from));
        let c = deb_command(DebTool::Dpkg, &p("/tmp/x.deb"));
        assert_eq!(c, ["pkexec", "dpkg", "-i", "/tmp/x.deb"].map(OsString::from));
    }

    /// Runs the helper synchronously against a process that already exited.
    fn helper(mode: &str, cur: &Path, new: &Path, relaunch: &[&str]) -> i32 {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("h.sh");
        std::fs::write(&script, HELPER_SCRIPT).unwrap();
        let mut gone = Command::new("/bin/sh").arg("-c").arg("exit 0").spawn().unwrap();
        let pid = gone.id();
        gone.wait().unwrap();
        let st = Command::new("/bin/sh").arg(&script).arg(mode).arg(pid.to_string()).arg(cur).arg(new).args(relaunch).stderr(Stdio::null()).status().unwrap();
        st.code().unwrap_or(-1)
    }

    /// The real macOS path: a signed bundle in a disk image is staged with
    /// hdiutil + ditto + codesign, then swapped by the helper.
    #[cfg(target_os = "macos")]
    #[test]
    fn stages_a_bundle_from_a_real_dmg() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let app = src.join("OpenDecks Session.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::copy("/usr/bin/true", app.join("Contents/MacOS/opendecks")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>CFBundleExecutable</key><string>opendecks</string><key>CFBundleIdentifier</key><string>org.opendecks.test</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>0.2.0</string></dict></plist>"#,
        )
        .unwrap();
        assert!(Command::new("codesign").args(["--force", "--sign", "-"]).arg(&app).stderr(Stdio::null()).status().unwrap().success());
        let dmg = d.path().join("OpenDecks-Session-0.2.0-macos-arm64.dmg");
        let made = Command::new("hdiutil").args(["create", "-quiet", "-fs", "HFS+", "-format", "UDZO", "-volname", "OpenDecks Session", "-srcfolder"]).arg(&src).arg(&dmg).status();
        if !made.is_ok_and(|s| s.success()) {
            eprintln!("hdiutil create unavailable here; skipping");
            return;
        }
        let installed = d.path().join("Applications");
        let cur = bundle(&installed, "OpenDecks Session.app", "old");
        let staging = staging_path(&cur, "0.2.0");
        stage_from_dmg(&dmg, &d.path().join("mnt"), &staging).unwrap();
        assert!(staging.join("Contents/MacOS/opendecks").is_file());
        assert!(!d.path().join("mnt").exists(), "image left mounted");
        assert_eq!(helper("swap", &cur, &staging, &[]), 0);
        assert!(cur.join("Contents/Info.plist").is_file());
        assert!(!staging.exists());
        // A corrupt image fails cleanly and stages nothing.
        let bad = d.path().join("bad.dmg");
        std::fs::write(&bad, b"not a disk image").unwrap();
        let staging2 = staging_path(&cur, "0.3.0");
        assert!(stage_from_dmg(&bad, &d.path().join("mnt2"), &staging2).is_err());
        assert!(!staging2.exists());
    }

    fn bundle(dir: &Path, name: &str, content: &str) -> PathBuf {
        let b = dir.join(name);
        std::fs::create_dir_all(b.join("Contents/MacOS")).unwrap();
        std::fs::write(b.join("Contents/MacOS/opendecks"), content).unwrap();
        b
    }

    fn wait_for(p: &Path) -> bool {
        for _ in 0..100 {
            if p.exists() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn helper_swaps_a_bundle_and_relaunches() {
        let d = tempfile::tempdir().unwrap();
        let cur = bundle(d.path(), "OpenDecks Session.app", "old");
        let new = bundle(d.path(), ".OpenDecks Session.app.update-0.2.0", "new");
        let marker = d.path().join("relaunched");
        let code = helper("swap", &cur, &new, &["touch", marker.to_str().unwrap()]);
        assert_eq!(code, 0);
        assert_eq!(std::fs::read_to_string(cur.join("Contents/MacOS/opendecks")).unwrap(), "new");
        assert!(!new.exists());
        assert!(!d.path().join("OpenDecks Session.app.old-update").exists());
        assert!(wait_for(&marker), "not relaunched");
    }

    #[test]
    fn helper_keeps_the_old_bundle_when_the_new_one_is_missing() {
        let d = tempfile::tempdir().unwrap();
        let cur = bundle(d.path(), "OpenDecks Session.app", "old");
        let marker = d.path().join("relaunched");
        let code = helper("swap", &cur, &d.path().join("missing.app"), &["touch", marker.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert_eq!(std::fs::read_to_string(cur.join("Contents/MacOS/opendecks")).unwrap(), "old");
        // The old version is started again.
        assert!(wait_for(&marker));
    }

    #[test]
    fn helper_rolls_back_when_the_second_rename_fails() {
        let d = tempfile::tempdir().unwrap();
        let cur = bundle(d.path(), "A.app", "old");
        // A new "bundle" in a folder we cannot write to cannot be moved.
        let locked = d.path().join("locked");
        let new = bundle(&locked, "B.app", "new");
        let mut perm = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o555);
        std::fs::set_permissions(&locked, perm.clone()).unwrap();
        let code = helper("swap", &cur, &new, &[]);
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&locked, perm).unwrap();
        assert_eq!(code, 1);
        assert_eq!(std::fs::read_to_string(cur.join("Contents/MacOS/opendecks")).unwrap(), "old");
        assert!(!d.path().join("A.app.old-update").exists());
    }

    #[test]
    fn helper_swaps_an_appimage_file() {
        let d = tempfile::tempdir().unwrap();
        let cur = d.path().join("OpenDecks-Session-0.0.1-x86_64.AppImage");
        let new = staging_path(&cur, "0.2.0");
        std::fs::write(&cur, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        assert_eq!(helper("swap", &cur, &new, &[]), 0);
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "new");
        assert!(!new.exists());
        let mode = std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&cur).unwrap().permissions());
        assert_ne!(mode & 0o111, 0, "not executable");
    }

    #[test]
    fn helper_wait_mode_only_relaunches() {
        let d = tempfile::tempdir().unwrap();
        let marker = d.path().join("relaunched");
        assert_eq!(helper("wait", Path::new(""), Path::new(""), &["touch", marker.to_str().unwrap()]), 0);
        assert!(wait_for(&marker));
    }

    #[test]
    fn helper_waits_for_the_app_to_exit() {
        let d = tempfile::tempdir().unwrap();
        let cur = d.path().join("a");
        let new = d.path().join("b");
        std::fs::write(&cur, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        let script = d.path().join("h.sh");
        std::fs::write(&script, HELPER_SCRIPT).unwrap();
        let mut app = Command::new("sleep").arg("0.6").spawn().unwrap();
        let mut h = Command::new("/bin/sh").arg(&script).arg("swap").arg(app.id().to_string()).arg(&cur).arg(&new).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "old", "swapped while the app still ran");
        app.wait().unwrap();
        assert!(h.wait().unwrap().success());
        assert_eq!(std::fs::read_to_string(&cur).unwrap(), "new");
    }
}
