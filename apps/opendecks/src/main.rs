//! OpenDecks Session application entry point.
//!
//! Settings are made in the Preferences window (⌘, / Ctrl+,) and stored in
//! the library; these options override them for one run.
//!
//! Options:
//!   --library <path>    library database (default: Preferences > Advanced,
//!                       else the platform data directory)
//!   --profile lite|full performance profile (default: detected)
//!   --kiosk             full-screen Perform for a Raspberry Pi
//!   --scale <factor>    UI scale factor
//!   --lang en|de        interface language (default: from the system)
//!   --keys camelot|openkey|musical
//!   --window WxH        fixed window size (emulate a Pi display)
//!   --colorblind        colour-blind-safe palette
//!   --high-contrast     high-contrast palette (bright light)
//!   --no-motion         no animations (always off in the Lite profile)
//!   --appearance dark|light|system
//!                       colour appearance (system follows the OS)

use od_core::KeyNotation;

fn detect_lite() -> bool {
    // Raspberry Pi and other small ARM boards run the Lite profile.
    std::fs::read_to_string("/proc/device-tree/model").is_ok_and(|m| m.contains("Raspberry Pi"))
        || std::thread::available_parallelism().is_ok_and(|n| n.get() <= 4)
            && cfg!(target_os = "linux")
            && cfg!(target_arch = "aarch64")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let mut args = std::env::args().skip(1);
    let mut library = None;
    let mut config = od_ui::AppConfig::new(Default::default());
    config.lite = detect_lite();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--library" => library = args.next().map(Into::into),
            "--profile" => config.lite = args.next().as_deref() == Some("lite"),
            "--kiosk" => config.kiosk = true,
            "--scale" => config.overrides.scale = args.next().and_then(|s| s.parse().ok()),
            "--lang" => config.overrides.language = args.next(),
            "--colorblind" => config.overrides.colorblind_safe = Some(true),
            "--high-contrast" => config.overrides.high_contrast = Some(true),
            "--no-motion" => config.overrides.reduced_motion = Some(true),
            "--appearance" => config.overrides.appearance = args.next(),
            "--window" => {
                config.window = args.next().and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                })
            }
            "--keys" => {
                config.overrides.key_notation = Some(match args.next().as_deref() {
                    Some("openkey") => KeyNotation::OpenKey,
                    Some("musical") => KeyNotation::Musical,
                    _ => KeyNotation::Camelot,
                })
            }
            "--help" | "-h" => {
                println!(
                    "opendecks [--library PATH] [--profile lite|full] [--kiosk] [--scale F] [--lang en|de] [--keys camelot|openkey|musical] [--colorblind] [--high-contrast] [--no-motion] [--appearance dark|light|system] [--window WxH]"
                );
                return Ok(());
            }
            other => anyhow::bail!("unknown option {other}"),
        }
    }
    config.library_path = od_ui::library_path(library);
    tracing::info!(library = %config.library_path.display(), lite = config.lite, kiosk = config.kiosk, "starting OpenDecks Session");
    od_ui::run(config)
}
