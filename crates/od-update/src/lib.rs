//! Update check and installer (SPEC §2 network exception, §14.1).
//!
//! The only network access of the app: `GET` on the public GitHub releases
//! API of the project and, when the user installs, the release's asset and
//! `SHA256SUMS.txt`. The User-Agent is `OpenDecks-Session/<version>`; no
//! other identifiers, no cookies, no tokens.
//!
//! Flow: [`Updater::check`] → an [`Update`] (newest allowed release, the
//! asset for this install and the [`Strategy`]) → [`Updater::prepare`]
//! (download with resume, SHA-256 against `SHA256SUMS.txt`, staging) →
//! [`Prepared`]: either a [`Finish`] to run when the app quits (swap and
//! relaunch), or done already (DMG / release page opened). See
//! `docs/updates.md`.

pub mod checksum;
pub mod download;
pub mod http;
pub mod install;
pub mod platform;
pub mod release;
pub mod version;

pub use http::{Http, UreqHttp};
pub use install::{Finish, Probe, Strategy};
pub use platform::{Arch, InstallKind, Os};
pub use release::{Asset, Policy, Release};
pub use version::Version;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// The releases API of the project (public, unauthenticated).
pub const RELEASES_API: &str =
    "https://api.github.com/repos/nurkert/opendecks-session/releases?per_page=30";
/// The human-readable releases page.
pub const RELEASES_PAGE: &str = "https://github.com/nurkert/opendecks-session/releases";
/// The checksum list every release carries (`.github/workflows/release.yml`).
pub const SUMS_NAME: &str = "SHA256SUMS.txt";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network: {0}")]
    Network(String),
    #[error("HTTP {0} from {1}")]
    Status(u16, String),
    #[error("unreadable answer: {0}")]
    Parse(String),
    #[error("checksum mismatch for {file}: expected {expected}, got {got}")]
    Checksum {
        file: String,
        expected: String,
        got: String,
    },
    #[error("the release has no checksum for {0}")]
    NoChecksum(String),
    #[error("install: {0}")]
    Install(String),
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A release newer than the running version, ready to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    pub tag: String,
    pub prerelease: bool,
    /// Release notes (Markdown) and the release page.
    pub notes: String,
    pub page_url: String,
    /// The file for this install kind and the checksum list.
    pub asset: Option<Asset>,
    pub sums: Option<Asset>,
    pub strategy: Strategy,
}

/// The result of [`Updater::prepare`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prepared {
    /// Staged: run this when the app quits (it restarts the app).
    Restart(Finish),
    /// The disk image was opened for the user to drag the app over.
    OpenedDmg(PathBuf),
    /// The release page was opened in the browser.
    OpenedPage(String),
}

/// What the updater knows about this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
    pub kind: InstallKind,
    pub probe: Probe,
    /// Arguments to restart with (AppImage / .deb).
    pub args: Vec<std::ffi::OsString>,
}

impl Target {
    pub fn detect() -> Self {
        let kind = platform::detect();
        let probe = Probe::of(&kind);
        Self {
            os: Os::current(),
            arch: Arch::current(),
            kind,
            probe,
            args: std::env::args_os().skip(1).collect(),
        }
    }
}

pub struct Updater {
    pub http: Arc<dyn Http>,
    pub current: Version,
    pub target: Target,
    /// Downloads and the helper script live here.
    pub cache: PathBuf,
    pub api_url: String,
}

impl Updater {
    /// The real updater for this process.
    pub fn new(cache: PathBuf) -> Self {
        let current = version::current();
        Self {
            http: Arc::new(UreqHttp::new(&current.to_string())),
            current,
            target: Target::detect(),
            cache,
            api_url: RELEASES_API.into(),
        }
    }

    /// Asks GitHub for a newer release. `Ok(None)`: up to date.
    pub fn check(&self, policy: &Policy) -> Result<Option<Update>, Error> {
        let json = http::get_text(&*self.http, &self.api_url, 8 << 20)?;
        let releases = release::parse_releases(&json)?;
        Ok(self.pick(&releases, policy))
    }

    /// The update among `releases` (no network).
    pub fn pick(&self, releases: &[Release], policy: &Policy) -> Option<Update> {
        let (rel, version) = release::newest_update(releases, &self.current, policy)?;
        let t = &self.target;
        let asset = platform::select_asset(rel, t.os, t.arch, &t.kind).cloned();
        let sums = rel.sums().cloned();
        let strategy = install::strategy(&t.kind, asset.is_some() && sums.is_some(), t.probe);
        Some(Update {
            version,
            tag: rel.tag_name.clone(),
            prerelease: rel.is_prerelease(),
            notes: rel.body.clone().unwrap_or_default(),
            page_url: if rel.html_url.is_empty() {
                RELEASES_PAGE.into()
            } else {
                rel.html_url.clone()
            },
            asset,
            sums,
            strategy,
        })
    }

    /// Downloads, verifies and stages `update`. Nothing of the running
    /// version changes here; on error everything staged is removed.
    pub fn prepare(
        &self,
        update: &Update,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<Prepared, Error> {
        let (asset, sums) = match (&update.strategy, &update.asset, &update.sums) {
            (Strategy::OpenReleasePage, _, _) => {
                install::open(update.page_url.as_ref())?;
                return Ok(Prepared::OpenedPage(update.page_url.clone()));
            }
            (_, Some(a), Some(s)) => (a, s),
            _ => return Err(Error::NoChecksum(update.tag.clone())),
        };
        let sums_text = http::get_text(&*self.http, &sums.browser_download_url, 1 << 20)?;
        let expected = checksum::parse_sums(&sums_text)
            .remove(&asset.name)
            .ok_or_else(|| Error::NoChecksum(asset.name.clone()))?;
        let size = (asset.size > 0).then_some(asset.size);
        let fetch = |dest: &Path, progress: &mut dyn FnMut(u64, Option<u64>)| {
            download::download_verified(
                &*self.http,
                &asset.browser_download_url,
                dest,
                size,
                &expected,
                progress,
                cancel,
            )
        };
        let v = update.version.to_string();
        match &update.strategy {
            Strategy::SwapMacBundle { bundle } => {
                let dmg = self.cache.join(&asset.name);
                fetch(&dmg, progress)?;
                let staging = install::staging_path(bundle, &v);
                install::stage_from_dmg(
                    &dmg,
                    &self.cache.join(format!("mount-{}", std::process::id())),
                    &staging,
                )?;
                let _ = std::fs::remove_file(&dmg);
                Ok(Prepared::Restart(Finish {
                    mode: "swap",
                    current: bundle.clone(),
                    new: staging,
                    relaunch: vec!["open".into(), bundle.clone().into_os_string()],
                }))
            }
            Strategy::OpenDmg => {
                let dmg = self.cache.join(&asset.name);
                fetch(&dmg, progress)?;
                install::open(dmg.as_os_str())?;
                Ok(Prepared::OpenedDmg(dmg))
            }
            Strategy::SwapAppImage { path } => {
                // Downloaded straight next to the AppImage (same volume).
                let staging = install::staging_path(path, &v);
                fetch(&staging, progress)?;
                set_executable(&staging)?;
                let mut relaunch = vec![path.clone().into_os_string()];
                relaunch.extend(self.target.args.iter().cloned());
                Ok(Prepared::Restart(Finish {
                    mode: "swap",
                    current: path.clone(),
                    new: staging,
                    relaunch,
                }))
            }
            Strategy::InstallDeb { tool } => {
                let deb = self.cache.join(&asset.name);
                fetch(&deb, progress)?;
                install::install_deb(*tool, &deb)?;
                let _ = std::fs::remove_file(&deb);
                let exe =
                    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/bin/opendecks"));
                let mut relaunch = vec![exe.into_os_string()];
                relaunch.extend(self.target.args.iter().cloned());
                Ok(Prepared::Restart(Finish {
                    mode: "wait",
                    current: PathBuf::new(),
                    new: PathBuf::new(),
                    relaunch,
                }))
            }
            Strategy::OpenReleasePage => unreachable!(),
        }
    }

    /// Starts the helper; the app must quit right after.
    pub fn finish(&self, f: &Finish) -> Result<(), Error> {
        f.spawn(&self.cache)
    }
}

fn set_executable(p: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(p)?.permissions();
    perm.set_mode(perm.mode() | 0o755);
    std::fs::set_permissions(p, perm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::sha256_bytes;
    use crate::http::mock::MockHttp;

    fn updater(http: MockHttp, kind: InstallKind, probe: Probe, os: Os, cache: &Path) -> Updater {
        Updater {
            http: Arc::new(http),
            current: Version::new(0, 0, 1),
            target: Target {
                os,
                arch: Arch::X86_64,
                kind,
                probe,
                args: vec!["--lang".into(), "de".into()],
            },
            cache: cache.to_path_buf(),
            api_url: "https://api.example.invalid/releases".into(),
        }
    }

    fn releases_json() -> String {
        include_str!("../tests/fixtures/releases.json").to_string()
    }

    #[test]
    fn check_finds_the_update_and_strategy_per_install() {
        let http =
            || MockHttp::default().with("https://api.example.invalid/releases", releases_json());
        let d = tempfile::tempdir().unwrap();
        let writable = Probe {
            dir_writable: true,
            ..Default::default()
        };
        let cases: Vec<(Os, InstallKind, Probe, Option<&str>, Strategy)> = vec![
            (
                Os::MacOs,
                InstallKind::MacBundle {
                    bundle: "/Applications/OpenDecks Session.app".into(),
                },
                writable,
                Some("OpenDecks-Session-0.2.0-macos-x86_64.dmg"),
                Strategy::SwapMacBundle {
                    bundle: "/Applications/OpenDecks Session.app".into(),
                },
            ),
            (
                Os::MacOs,
                InstallKind::MacBundle {
                    bundle: "/Volumes/X/OpenDecks Session.app".into(),
                },
                Probe::default(),
                Some("OpenDecks-Session-0.2.0-macos-x86_64.dmg"),
                Strategy::OpenDmg,
            ),
            (
                Os::Linux,
                InstallKind::AppImage {
                    path: "/home/me/OD.AppImage".into(),
                },
                writable,
                Some("OpenDecks-Session-0.2.0-x86_64.AppImage"),
                Strategy::SwapAppImage {
                    path: "/home/me/OD.AppImage".into(),
                },
            ),
            (
                Os::Linux,
                InstallKind::Deb,
                Probe {
                    pkexec: true,
                    apt_get: true,
                    dpkg: true,
                    ..Default::default()
                },
                Some("opendecks-session_0.2.0_amd64.deb"),
                Strategy::InstallDeb {
                    tool: install::DebTool::AptGet,
                },
            ),
            (
                Os::Linux,
                InstallKind::Deb,
                Probe {
                    apt_get: true,
                    dpkg: true,
                    ..Default::default()
                },
                Some("opendecks-session_0.2.0_amd64.deb"),
                Strategy::OpenReleasePage,
            ),
            (
                Os::Linux,
                InstallKind::Unknown,
                writable,
                None,
                Strategy::OpenReleasePage,
            ),
        ];
        for (os, kind, probe, asset, strategy) in cases {
            let u = updater(http(), kind.clone(), probe, os, d.path())
                .check(&Policy::default())
                .unwrap()
                .unwrap();
            assert_eq!(u.version, Version::new(0, 2, 0));
            assert_eq!(u.asset.as_ref().map(|a| a.name.as_str()), asset, "{kind:?}");
            assert_eq!(u.strategy, strategy, "{kind:?}");
            assert_eq!(u.sums.as_ref().unwrap().name, "SHA256SUMS.txt");
            assert!(u.page_url.ends_with("/releases/tag/v0.2.0"));
            assert_eq!(u.notes, "Release notes for 0.2.0");
        }
    }

    #[test]
    fn check_errors_and_up_to_date() {
        let d = tempfile::tempdir().unwrap();
        let u = updater(
            MockHttp::default(),
            InstallKind::Unknown,
            Probe::default(),
            Os::Linux,
            d.path(),
        );
        assert!(matches!(
            u.check(&Policy::default()),
            Err(Error::Status(404, _))
        ));
        let http = MockHttp::default().with("https://api.example.invalid/releases", "<html>");
        let u = updater(
            http,
            InstallKind::Unknown,
            Probe::default(),
            Os::Linux,
            d.path(),
        );
        assert!(matches!(u.check(&Policy::default()), Err(Error::Parse(_))));
        let http =
            MockHttp::default().with("https://api.example.invalid/releases", releases_json());
        let mut u = updater(
            http,
            InstallKind::Unknown,
            Probe::default(),
            Os::Linux,
            d.path(),
        );
        u.current = Version::new(0, 2, 0);
        assert_eq!(u.check(&Policy::default()).unwrap(), None);
    }

    /// A release whose AppImage is served by the mock.
    fn appimage_release(payload: &[u8], sums: &str) -> (MockHttp, Vec<Release>) {
        let base = "https://github.com/nurkert/opendecks-session/releases/download/v0.2.0";
        let name = "OpenDecks-Session-0.2.0-x86_64.AppImage";
        let rel = Release {
            tag_name: "v0.2.0".into(),
            name: None,
            draft: false,
            prerelease: false,
            html_url: "https://github.com/nurkert/opendecks-session/releases/tag/v0.2.0".into(),
            body: None,
            published_at: None,
            assets: vec![
                Asset {
                    name: name.into(),
                    size: payload.len() as u64,
                    browser_download_url: format!("{base}/{name}"),
                },
                Asset {
                    name: SUMS_NAME.into(),
                    size: 0,
                    browser_download_url: format!("{base}/{SUMS_NAME}"),
                },
            ],
        };
        let http = MockHttp::default()
            .with(&format!("{base}/{name}"), payload.to_vec())
            .with(&format!("{base}/{SUMS_NAME}"), sums.to_string());
        (http, vec![rel])
    }

    #[test]
    fn prepare_stages_a_verified_appimage() {
        let d = tempfile::tempdir().unwrap();
        let current = d.path().join("OpenDecks-Session-0.0.1-x86_64.AppImage");
        std::fs::write(&current, b"old").unwrap();
        let payload = b"new appimage".repeat(1000);
        let sums = format!(
            "{}  OpenDecks-Session-0.2.0-x86_64.AppImage\n{}  other.deb\n",
            sha256_bytes(&payload),
            sha256_bytes(b"x")
        );
        let (http, rels) = appimage_release(&payload, &sums);
        let kind = InstallKind::AppImage {
            path: current.clone(),
        };
        let u = updater(
            http,
            kind,
            Probe {
                dir_writable: true,
                ..Default::default()
            },
            Os::Linux,
            &d.path().join("cache"),
        );
        let up = u.pick(&rels, &Policy::default()).unwrap();
        let mut last = (0, None);
        let prepared = u
            .prepare(&up, &mut |a, b| last = (a, b), &AtomicBool::new(false))
            .unwrap();
        let Prepared::Restart(f) = prepared else {
            panic!("{prepared:?}")
        };
        assert_eq!(f.mode, "swap");
        assert_eq!(f.current, current);
        assert_eq!(
            f.new,
            d.path()
                .join(".OpenDecks-Session-0.0.1-x86_64.AppImage.update-0.2.0")
        );
        assert_eq!(std::fs::read(&f.new).unwrap(), payload);
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            std::fs::metadata(&f.new).unwrap().permissions().mode() & 0o111,
            0
        );
        assert_eq!(
            f.relaunch,
            vec![
                current.clone().into_os_string(),
                "--lang".into(),
                "de".into()
            ]
        );
        assert_eq!(last, (payload.len() as u64, Some(payload.len() as u64)));
        // The running version is untouched until the helper runs.
        assert_eq!(std::fs::read(&current).unwrap(), b"old");
    }

    #[test]
    fn prepare_refuses_a_tampered_download() {
        let d = tempfile::tempdir().unwrap();
        let current = d.path().join("OD.AppImage");
        std::fs::write(&current, b"old").unwrap();
        let sums = format!(
            "{}  OpenDecks-Session-0.2.0-x86_64.AppImage\n",
            sha256_bytes(b"what was released")
        );
        let (http, rels) = appimage_release(b"something else", &sums);
        let u = updater(
            http,
            InstallKind::AppImage {
                path: current.clone(),
            },
            Probe {
                dir_writable: true,
                ..Default::default()
            },
            Os::Linux,
            d.path(),
        );
        let up = u.pick(&rels, &Policy::default()).unwrap();
        let e = u
            .prepare(&up, &mut |_, _| {}, &AtomicBool::new(false))
            .unwrap_err();
        assert!(matches!(e, Error::Checksum { .. }), "{e}");
        // Nothing staged, nothing replaced.
        let names: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["OD.AppImage".to_string()]);
        assert_eq!(std::fs::read(&current).unwrap(), b"old");
    }

    #[test]
    fn prepare_needs_a_checksum_for_the_asset() {
        let d = tempfile::tempdir().unwrap();
        let current = d.path().join("OD.AppImage");
        let (http, rels) = appimage_release(
            b"payload",
            &format!("{}  some-other-file\n", sha256_bytes(b"payload")),
        );
        let u = updater(
            http,
            InstallKind::AppImage { path: current },
            Probe {
                dir_writable: true,
                ..Default::default()
            },
            Os::Linux,
            d.path(),
        );
        let up = u.pick(&rels, &Policy::default()).unwrap();
        assert!(matches!(
            u.prepare(&up, &mut |_, _| {}, &AtomicBool::new(false)),
            Err(Error::NoChecksum(_))
        ));
        // A release without SHA256SUMS.txt is not installed in place at all.
        let mut rels = rels;
        rels[0].assets.retain(|a| a.name != SUMS_NAME);
        assert_eq!(
            u.pick(&rels, &Policy::default()).unwrap().strategy,
            Strategy::OpenReleasePage
        );
    }
}
