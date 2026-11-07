//! SPEC §16 gates.
//!
//! - `features_toml_is_valid`, `feature_matrix_is_generated`: always
//!   enforced (the tables are consistent and refer to real tests).
//! - `feature_set_complete`: prints the gap report; fails on gaps only with
//!   `OD_PARITY_STRICT=1` (the final CI gate, see docs/parity.md).

use od_parity::{Inputs, MATRIX, check, matrix, workspace_root};

fn inputs() -> Inputs {
    Inputs::load(&workspace_root()).unwrap_or_else(|e| panic!("{e}"))
}

fn strict() -> bool {
    std::env::var("OD_PARITY_STRICT").is_ok_and(|v| v == "1")
}

#[test]
fn features_toml_is_valid() {
    let report = inputs().report();
    assert!(
        report.errors.is_empty(),
        "parity tables have {} error(s):\n{}",
        report.errors.len(),
        report.errors.join("\n")
    );
}

#[test]
fn feature_matrix_is_generated() {
    let i = inputs();
    let expected = matrix::render(&i.features);
    let current = std::fs::read_to_string(workspace_root().join(MATRIX)).unwrap_or_default();
    assert!(
        current == expected,
        "{MATRIX} is not generated from tests/parity/features.toml; run \
         `cargo run -p od-parity --bin parity -- matrix`"
    );
}

#[test]
fn feature_set_complete() {
    let strict = strict();
    let report = inputs().report();
    let text = check::render(&report, strict);
    println!("{text}");
    if strict {
        assert_eq!(
            report.gap_count(),
            0,
            "parity gaps remain (OD_PARITY_STRICT=1)"
        );
    }
}
