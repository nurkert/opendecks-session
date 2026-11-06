//! Locates the Rubber Band library (keylock) through pkg-config.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    if std::env::var_os("CARGO_FEATURE_RUBBERBAND").is_none() {
        return;
    }
    match pkg_config::Config::new()
        .atleast_version("3.0")
        .probe("rubberband")
    {
        Ok(_) => {}
        Err(e) => panic!(
            "Rubber Band library not found ({e}).\n\
             Install it (macOS: `brew install rubberband`, Debian/Raspberry Pi OS: \
             `sudo apt install librubberband-dev pkg-config`) or build od-engine with \
             `--no-default-features` to disable keylock."
        ),
    }
}
