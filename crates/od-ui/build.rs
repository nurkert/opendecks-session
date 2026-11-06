fn main() {
    println!("cargo:rerun-if-changed=lang/de/LC_MESSAGES/od-ui.po");
    let config = slint_build::CompilerConfiguration::new().with_bundled_translations("lang");
    slint_build::compile_with_config("ui/app.slint", config).expect("Slint build failed");
}
