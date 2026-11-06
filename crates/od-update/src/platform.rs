//! Where and how this copy of the app is installed, and which release asset
//! replaces it.
//!
//! | Install | Detected by | Asset |
//! |---|---|---|
//! | macOS `.app` | `current_exe` is `<X>.app/Contents/MacOS/<bin>` | `OpenDecks-Session-<v>-macos-<arm64\|x86_64>.dmg` |
//! | AppImage | `$APPIMAGE` names an existing file | `OpenDecks-Session-<v>-<x86_64\|aarch64>.AppImage` |
//! | `.deb` | binary in `/usr/bin` and owned by the `opendecks-session` package | `opendecks-session_<v>_<amd64\|arm64>.deb` |
//! | anything else | (source build, tar.gz, Raspberry Pi bundle) | none: the release page opens |

use crate::release::{Asset, Release};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Other,
}

impl Os {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            _ => Os::Other,
        }
    }
}

impl Arch {
    pub fn current() -> Self {
        match std::env::consts::ARCH {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            _ => Arch::Other,
        }
    }
}

/// How the running app was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// A macOS app bundle (`…/OpenDecks Session.app`).
    MacBundle { bundle: PathBuf },
    /// A Linux AppImage file.
    AppImage { path: PathBuf },
    /// The Debian package `opendecks-session`.
    Deb,
    /// Not updatable in place: open the release page.
    Unknown,
}

/// The Debian package name (packaging: `apps/opendecks/Cargo.toml`).
pub const DEB_PACKAGE: &str = "opendecks-session";
const DPKG_LIST: &str = "/var/lib/dpkg/info/opendecks-session.list";

/// The `.app` bundle that contains `exe`, if any.
pub fn mac_bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS" && contents.file_name()? == "Contents" && bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

/// True when dpkg's file list of our package names `exe`.
pub fn dpkg_list_owns(list: &str, exe: &Path) -> bool {
    let exe = exe.to_string_lossy();
    list.lines().any(|l| l.trim() == exe)
}

/// Decides the install kind from the facts (pure; [`detect`] gathers them).
/// `appimage` is `$APPIMAGE` when it names an existing file; `deb_owned`
/// is only asked for binaries in `/usr/bin`.
pub fn install_kind(os: Os, exe: &Path, appimage: Option<&Path>, deb_owned: impl FnOnce(&Path) -> bool) -> InstallKind {
    match os {
        Os::MacOs => match mac_bundle_of(exe) {
            Some(bundle) => InstallKind::MacBundle { bundle },
            None => InstallKind::Unknown,
        },
        Os::Linux => {
            if let Some(p) = appimage {
                return InstallKind::AppImage { path: p.to_path_buf() };
            }
            if exe.parent() == Some(Path::new("/usr/bin")) && deb_owned(exe) {
                return InstallKind::Deb;
            }
            InstallKind::Unknown
        }
        Os::Other => InstallKind::Unknown,
    }
}

/// The install kind of the running process.
pub fn detect() -> InstallKind {
    let exe = std::env::current_exe().ok().map(|p| p.canonicalize().unwrap_or(p)).unwrap_or_default();
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file());
    install_kind(Os::current(), &exe, appimage.as_deref(), |exe| {
        if let Ok(list) = std::fs::read_to_string(DPKG_LIST) {
            return dpkg_list_owns(&list, exe);
        }
        // `dpkg -S /usr/bin/opendecks` → "opendecks-session: /usr/bin/opendecks"
        std::process::Command::new("dpkg")
            .arg("-S")
            .arg(exe)
            .stderr(std::process::Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).lines().any(|l| l.split(':').next() == Some(DEB_PACKAGE)))
    })
}

/// The file-name prefix and suffix of the asset for this install, as
/// `packaging/` names them (the version sits between).
pub fn asset_pattern(os: Os, arch: Arch, kind: &InstallKind) -> Option<(&'static str, String)> {
    match (kind, os) {
        (InstallKind::MacBundle { .. }, Os::MacOs) => {
            let a = match arch {
                Arch::Aarch64 => "arm64",
                Arch::X86_64 => "x86_64",
                Arch::Other => return None,
            };
            Some(("OpenDecks-Session-", format!("-macos-{a}.dmg")))
        }
        (InstallKind::AppImage { .. }, Os::Linux) => {
            let a = match arch {
                Arch::Aarch64 => "aarch64",
                Arch::X86_64 => "x86_64",
                Arch::Other => return None,
            };
            Some(("OpenDecks-Session-", format!("-{a}.AppImage")))
        }
        (InstallKind::Deb, Os::Linux) => {
            let a = match arch {
                Arch::Aarch64 => "arm64",
                Arch::X86_64 => "amd64",
                Arch::Other => return None,
            };
            Some(("opendecks-session_", format!("_{a}.deb")))
        }
        _ => None,
    }
}

/// The asset of `release` that updates this install.
pub fn select_asset<'a>(release: &'a Release, os: Os, arch: Arch, kind: &InstallKind) -> Option<&'a Asset> {
    let (prefix, suffix) = asset_pattern(os, arch, kind)?;
    release.assets.iter().find(|a| a.name.starts_with(prefix) && a.name.ends_with(&suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::parse_releases;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn mac_bundle_from_the_executable() {
        assert_eq!(mac_bundle_of(&p("/Applications/OpenDecks Session.app/Contents/MacOS/opendecks")), Some(p("/Applications/OpenDecks Session.app")));
        assert_eq!(mac_bundle_of(&p("/Users/me/Apps/Renamed.app/Contents/MacOS/opendecks")), Some(p("/Users/me/Apps/Renamed.app")));
        assert_eq!(mac_bundle_of(&p("/Users/me/src/target/release/opendecks")), None);
        assert_eq!(mac_bundle_of(&p("/Contents/MacOS/opendecks")), None);
        assert_eq!(mac_bundle_of(&p("opendecks")), None);
    }

    #[test]
    fn install_kind_macos() {
        let never = |_: &Path| -> bool { panic!("dpkg asked on macOS") };
        assert_eq!(
            install_kind(Os::MacOs, &p("/Applications/OpenDecks Session.app/Contents/MacOS/opendecks"), None, never),
            InstallKind::MacBundle { bundle: p("/Applications/OpenDecks Session.app") }
        );
        assert_eq!(install_kind(Os::MacOs, &p("/Users/me/src/target/release/opendecks"), None, never), InstallKind::Unknown);
        // $APPIMAGE means nothing on macOS.
        assert_eq!(install_kind(Os::MacOs, &p("/opt/x/opendecks"), Some(&p("/tmp/a.AppImage")), never), InstallKind::Unknown);
    }

    #[test]
    fn install_kind_appimage_wins_on_linux() {
        // An AppImage mounts at /tmp/.mount_*/usr/bin/opendecks.
        let k = install_kind(Os::Linux, &p("/tmp/.mount_OpenDeXYZ/usr/bin/opendecks"), Some(&p("/home/me/Apps/OpenDecks-Session-0.0.1-x86_64.AppImage")), |_| true);
        assert_eq!(k, InstallKind::AppImage { path: p("/home/me/Apps/OpenDecks-Session-0.0.1-x86_64.AppImage") });
    }

    #[test]
    fn install_kind_deb() {
        assert_eq!(install_kind(Os::Linux, &p("/usr/bin/opendecks"), None, |_| true), InstallKind::Deb);
        // In /usr/bin but not from our package (e.g. copied there by hand).
        assert_eq!(install_kind(Os::Linux, &p("/usr/bin/opendecks"), None, |_| false), InstallKind::Unknown);
        // Elsewhere dpkg is not asked.
        let never = |_: &Path| -> bool { panic!("dpkg asked") };
        assert_eq!(install_kind(Os::Linux, &p("/usr/local/bin/opendecks"), None, never), InstallKind::Unknown);
        assert_eq!(install_kind(Os::Linux, &p("/opt/opendecks/opendecks"), None, never), InstallKind::Unknown);
        assert_eq!(install_kind(Os::Linux, &p("/home/me/opendecks-session-0.0.1-linux-x86_64/opendecks"), None, never), InstallKind::Unknown);
        assert_eq!(install_kind(Os::Other, &p("/usr/bin/opendecks"), None, |_| true), InstallKind::Unknown);
    }

    #[test]
    fn dpkg_list_parsing() {
        let list = "/.\n/usr\n/usr/bin\n/usr/bin/od-cli\n/usr/bin/opendecks\n/usr/share/opendecks\n";
        assert!(dpkg_list_owns(list, &p("/usr/bin/opendecks")));
        assert!(!dpkg_list_owns(list, &p("/usr/bin/opendeck")));
        assert!(!dpkg_list_owns("", &p("/usr/bin/opendecks")));
    }

    #[test]
    fn asset_per_platform_and_install() {
        let r = parse_releases(crate::release::tests::FIXTURE).unwrap();
        let rel = r.iter().find(|r| r.tag_name == "v0.0.1").unwrap();
        let mac = InstallKind::MacBundle { bundle: p("/Applications/OpenDecks Session.app") };
        let app = InstallKind::AppImage { path: p("/x.AppImage") };
        let name = |os, arch, k: &InstallKind| select_asset(rel, os, arch, k).map(|a| a.name.as_str());
        assert_eq!(name(Os::MacOs, Arch::Aarch64, &mac), Some("OpenDecks-Session-0.0.1-macos-arm64.dmg"));
        assert_eq!(name(Os::MacOs, Arch::X86_64, &mac), Some("OpenDecks-Session-0.0.1-macos-x86_64.dmg"));
        assert_eq!(name(Os::Linux, Arch::X86_64, &app), Some("OpenDecks-Session-0.0.1-x86_64.AppImage"));
        assert_eq!(name(Os::Linux, Arch::Aarch64, &app), Some("OpenDecks-Session-0.0.1-aarch64.AppImage"));
        assert_eq!(name(Os::Linux, Arch::X86_64, &InstallKind::Deb), Some("opendecks-session_0.0.1_amd64.deb"));
        assert_eq!(name(Os::Linux, Arch::Aarch64, &InstallKind::Deb), Some("opendecks-session_0.0.1_arm64.deb"));
        // Unknown installs, odd architectures and mismatched OS get nothing.
        assert_eq!(name(Os::Linux, Arch::X86_64, &InstallKind::Unknown), None);
        assert_eq!(name(Os::MacOs, Arch::Other, &mac), None);
        assert_eq!(name(Os::Linux, Arch::X86_64, &mac), None);
        assert_eq!(name(Os::MacOs, Arch::X86_64, &InstallKind::Deb), None);
        // The Pi bundle and the tar.gz are never picked.
        for os in [Os::Linux, Os::MacOs] {
            for arch in [Arch::X86_64, Arch::Aarch64] {
                for k in [&mac, &app, &InstallKind::Deb] {
                    if let Some(n) = name(os, arch, k) {
                        assert!(!n.ends_with(".tar.gz") && !n.starts_with("opendecks-pi"), "{n}");
                    }
                }
            }
        }
    }
}
