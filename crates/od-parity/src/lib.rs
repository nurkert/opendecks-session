//! Parity and superiority verification harness (SPEC §16).
//!
//! Reads the three tables under `tests/parity/` (`features.toml`,
//! `views.toml`, `flows.toml`), discovers the workspace's tests from the
//! sources, validates the tables against each other and against the code,
//! renders `docs/feature-matrix.md` and prints the gap report. See
//! `docs/parity.md`.

pub mod check;
pub mod discover;
pub mod labels;
pub mod matrix;
pub mod model;
pub mod ui_results;

use std::path::{Path, PathBuf};

/// The workspace root (two levels above this crate).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/od-parity lives two levels below the workspace root")
        .to_path_buf()
}

pub const FEATURES: &str = "tests/parity/features.toml";
pub const VIEWS: &str = "tests/parity/views.toml";
pub const FLOWS: &str = "tests/parity/flows.toml";
pub const MATRIX: &str = "docs/feature-matrix.md";

/// Everything the checker needs, loaded from a workspace root.
pub struct Inputs {
    pub features: model::FeaturesFile,
    pub views: model::ViewsFile,
    pub flows: model::FlowsFile,
    pub tests: discover::TestIndex,
    pub ui_labels: std::collections::BTreeSet<String>,
    /// Results of the headless UI suites (`od-ui`), if they have run.
    pub ui_results: Option<ui_results::UiResults>,
}

impl Inputs {
    pub fn load(root: &Path) -> Result<Self, String> {
        Ok(Self {
            features: model::load(&root.join(FEATURES))?,
            views: model::load(&root.join(VIEWS))?,
            flows: model::load(&root.join(FLOWS))?,
            tests: discover::discover(root).map_err(|e| format!("test discovery: {e}"))?,
            ui_labels: labels::labels_in_ui_sources(root),
            ui_results: ui_results::UiResults::load(root),
        })
    }

    pub fn report(&self) -> check::Report {
        check::check(
            &self.features,
            &self.views,
            &self.flows,
            &self.tests,
            &self.ui_labels,
            self.ui_results.as_ref(),
        )
    }
}
