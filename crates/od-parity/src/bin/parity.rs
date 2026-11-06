//! `parity` — the SPEC §16 harness on the command line.
//!
//! ```text
//! cargo run -p od-parity --bin parity -- check [--strict] [--out FILE]
//! cargo run -p od-parity --bin parity -- matrix [--check]
//! cargo run -p od-parity --bin parity -- list-tests [FILTER]
//! ```

use od_parity::{Inputs, MATRIX, check, matrix, workspace_root};
use std::process::ExitCode;

const USAGE: &str = "usage: parity <check [--strict] [--out FILE] | matrix [--check] | list-tests [FILTER]> [--root DIR]";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = workspace_root();
    if let Some(i) = args.iter().position(|a| a == "--root") {
        if i + 1 >= args.len() {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        root = args.remove(i + 1).into();
        args.remove(i);
    }
    let Some(cmd) = args.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let flag = |f: &str| args.iter().any(|a| a == f);
    if cmd == "list-tests" {
        let index = match od_parity::discover::discover(&root) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("parity: test discovery: {e}");
                return ExitCode::FAILURE;
            }
        };
        let filter = args.get(1).map(String::as_str).unwrap_or("");
        for t in index.tests.values().filter(|t| t.id.contains(filter)) {
            let ignored = if t.ignored { "  (ignored)" } else { "" };
            println!("{}  {}:{}{ignored}", t.id, t.file.display(), t.line);
        }
        return ExitCode::SUCCESS;
    }
    let inputs = match Inputs::load(&root) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("parity: {e}");
            return ExitCode::FAILURE;
        }
    };
    match cmd.as_str() {
        "check" => {
            let strict = flag("--strict") || std::env::var("OD_PARITY_STRICT").is_ok_and(|v| v == "1");
            let report = inputs.report();
            let text = check::render(&report, strict);
            print!("{text}");
            if let Some(i) = args.iter().position(|a| a == "--out") {
                let Some(path) = args.get(i + 1) else {
                    eprintln!("{USAGE}");
                    return ExitCode::from(2);
                };
                if let Err(e) = std::fs::write(path, &text) {
                    eprintln!("parity: writing {path}: {e}");
                    return ExitCode::FAILURE;
                }
            }
            if !report.errors.is_empty() || (strict && report.gap_count() > 0) {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        "matrix" => {
            let text = matrix::render(&inputs.features);
            let path = root.join(MATRIX);
            if flag("--check") {
                let current = std::fs::read_to_string(&path).unwrap_or_default();
                if current == text {
                    println!("{MATRIX} is up to date");
                    ExitCode::SUCCESS
                } else {
                    eprintln!("{MATRIX} is out of date; run `cargo run -p od-parity --bin parity -- matrix`");
                    ExitCode::FAILURE
                }
            } else if let Err(e) = std::fs::write(&path, text) {
                eprintln!("parity: writing {}: {e}", path.display());
                ExitCode::FAILURE
            } else {
                println!("wrote {MATRIX}");
                println!("{}", matrix::summary_line(&inputs.features));
                ExitCode::SUCCESS
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
