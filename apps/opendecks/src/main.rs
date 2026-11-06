//! OpenDecks Session application entry point.
//!
//! Options:
//!   --library <path>    library database (default: platform data directory)
//!   --profile lite|full performance profile (default: detected)
//!   --kiosk             full-screen Perform for a Raspberry Pi
//!   --scale <factor>    UI scale factor
//!   --lang en|de        interface language (default: from the system)
//!   --keys camelot|openkey|musical
//!   --window WxH        fixed window size (emulate a Pi display)
//!   --colorblind        colour-blind-safe palette
//!   --high-contrast     high-contrast palette (bright light)
//!   --no-motion         no animations (always off in the Lite profile)

use od_core::KeyNotation;

fn detect_lite() -> bool {
    // Raspberry Pi and other small ARM boards run the Lite profile.
    std::fs::read_to_string("/proc/device-tree/model").is_ok_and(|m| m.contains("Raspberry Pi"))
        || std::thread::available_parallelism().is_ok_and(|n| n.get() <= 4) && cfg!(target_os = "linux") && cfg!(target_arch = "aarch64")
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    let mut args = std::env::args().skip(1);
    let mut library = od_library::Library::default_path();
    let mut lite = detect_lite();
    let mut kiosk = false;
    let mut lang = std::env::var("LANG").unwrap_or_default();
    let mut keys = KeyNotation::Camelot;
    let mut window = None;
    let (mut colorblind_safe, mut high_contrast, mut motion) = (false, false, true);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--library" => library = args.next().map(Into::into).unwrap_or(library),
            "--profile" => lite = args.next().as_deref() == Some("lite"),
            "--kiosk" => kiosk = true,
            "--scale" => {
                if let Some(s) = args.next() {
                    // SAFETY: set before any thread is started.
                    unsafe { std::env::set_var("SLINT_SCALE_FACTOR", s) };
                }
            }
            "--lang" => lang = args.next().unwrap_or_default(),
            "--colorblind" => colorblind_safe = true,
            "--high-contrast" => high_contrast = true,
            "--no-motion" => motion = false,
            "--window" => {
                window = args.next().and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                })
            }
            "--keys" => {
                keys = match args.next().as_deref() {
                    Some("openkey") => KeyNotation::OpenKey,
                    Some("musical") => KeyNotation::Musical,
                    _ => KeyNotation::Camelot,
                }
            }
            "--help" | "-h" => {
                println!("opendecks [--library PATH] [--profile lite|full] [--kiosk] [--scale F] [--lang en|de] [--keys camelot|openkey|musical] [--colorblind] [--high-contrast] [--no-motion] [--window WxH]");
                return Ok(());
            }
            other => anyhow::bail!("unknown option {other}"),
        }
    }
    tracing::info!(library = %library.display(), lite, kiosk, "starting OpenDecks Session");
    od_ui::run(od_ui::AppConfig { library_path: library, lite, kiosk, key_notation: keys, language: lang, window, colorblind_safe, high_contrast, motion })
}
