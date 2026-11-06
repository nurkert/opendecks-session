//! Manual check against the real GitHub API (network!):
//!
//! ```sh
//! cargo run -p od-update --example check -- [--pre] [--as 0.0.0] [--pretend X.app|X.AppImage|deb] [--verify]
//! ```
//!
//! Prints the install kind, the update found, its asset and strategy. With
//! `--verify` it also downloads the asset into a temporary folder and checks
//! it against `SHA256SUMS.txt` (nothing is installed).

use od_update::{Policy, Updater, Version};
use std::sync::atomic::AtomicBool;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cache = std::env::temp_dir().join(format!("od-update-check-{}", std::process::id()));
    let mut u = Updater::new(cache.clone());
    if let Some(i) = args.iter().position(|a| a == "--as") {
        u.current = Version::parse(args.get(i + 1).ok_or("--as needs a version")?)?;
    }
    // Pretend to be an installed bundle / AppImage / .deb (asset choice only).
    if let Some(i) = args.iter().position(|a| a == "--pretend") {
        let p = std::path::PathBuf::from(args.get(i + 1).ok_or("--pretend needs a path or \"deb\"")?);
        u.target.kind = match p.extension().and_then(|e| e.to_str()) {
            Some("app") => od_update::InstallKind::MacBundle { bundle: p },
            Some("AppImage") => od_update::InstallKind::AppImage { path: p },
            _ => od_update::InstallKind::Deb,
        };
        u.target.probe = od_update::Probe::of(&u.target.kind);
    }
    let policy = Policy { include_prereleases: args.iter().any(|a| a == "--pre"), skip: None };
    println!("running {} as {:?}, probe {:?}", u.current, u.target.kind, u.target.probe);
    let Some(up) = u.check(&policy)? else {
        println!("up to date");
        return Ok(());
    };
    println!("update {} ({}), strategy {:?}", up.version, up.page_url, up.strategy);
    println!("asset {:?}", up.asset.as_ref().map(|a| &a.name));
    // Download, verify and stage next to a (pretend) bundle / AppImage in a
    // writable folder; prints the helper call instead of running it.
    if args.iter().any(|a| a == "--stage") {
        if !matches!(up.strategy, od_update::Strategy::SwapMacBundle { .. } | od_update::Strategy::SwapAppImage { .. }) {
            return Err("--stage needs --pretend with a writable .app or .AppImage path".into());
        }
        let r = u.prepare(&up, &mut |_, _| {}, &AtomicBool::new(false))?;
        println!("prepared: {r:?}");
        return Ok(());
    }
    if args.iter().any(|a| a == "--verify") {
        let a = up.asset.as_ref().ok_or("no asset for this install")?;
        let sums = od_update::http::get_text(&*u.http, &up.sums.as_ref().ok_or("no SHA256SUMS.txt")?.browser_download_url, 1 << 20)?;
        let expected = od_update::checksum::parse_sums(&sums).remove(&a.name).ok_or("asset not in SHA256SUMS.txt")?;
        let dest = cache.join(&a.name);
        let mut last = 0;
        od_update::download::download_verified(
            &*u.http,
            &a.browser_download_url,
            &dest,
            Some(a.size),
            &expected,
            &mut |d, t| {
                let pct = t.map(|t| d * 100 / t.max(1)).unwrap_or(0);
                if pct >= last + 10 {
                    last = pct;
                    println!("  {pct} %");
                }
            },
            &AtomicBool::new(false),
        )?;
        println!("verified {} ({expected})", dest.display());
        let _ = std::fs::remove_dir_all(&cache);
    }
    Ok(())
}
