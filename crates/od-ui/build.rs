fn main() {
    println!("cargo:rerun-if-changed=lang/de/LC_MESSAGES/od-ui.po");
    // Element debug info lets the headless UI tests (Slint testing backend)
    // find elements by accessible label; release builds leave it out.
    println!("cargo:rerun-if-env-changed=PROFILE");
    let debug_info = std::env::var("PROFILE").map(|p| p != "release").unwrap_or(true);
    let config = slint_build::CompilerConfiguration::new().with_bundled_translations("lang").with_debug_info(debug_info);
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
