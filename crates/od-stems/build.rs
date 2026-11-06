// `onnx` is set when neural separation is both requested (feature
// `separation`) and possible: ONNX Runtime ships no prebuilt binaries for
// Intel Macs, so there the crate keeps its API but separation reports
// `Error::Unsupported` (cached stems still play).
fn main() {
    println!("cargo::rustc-check-cfg=cfg(onnx)");
    let target = std::env::var("TARGET").unwrap_or_default();
    let supported = target != "x86_64-apple-darwin";
    if std::env::var_os("CARGO_FEATURE_SEPARATION").is_some() && supported {
        println!("cargo::rustc-cfg=onnx");
    }
}
