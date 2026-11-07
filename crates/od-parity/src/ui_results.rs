//! Results of the headless UI suites in `od-ui` (SPEC §16.3, §16.4).
//!
//! The view completeness test (`od-ui::ui_tests::views::view_completeness_*`)
//! writes `target/parity/views-<size>.json`, the flow tests
//! (`od-ui::ui_tests::flows::*`) write `target/parity/flows/<id>.json`.
//! `parity check` summarises them when present (run
//! `cargo test -p od-ui ui_tests` first); without them the report says the
//! UI suites have not run.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

pub const DIR: &str = "target/parity";

#[derive(Debug, Clone, Deserialize)]
pub struct ViewRun {
    pub id: String,
    pub available: bool,
    pub passed: bool,
    #[serde(default)]
    pub reason: String,
    pub required: usize,
    #[serde(default)]
    pub present: Vec<String>,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default)]
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SizeRun {
    pub size: String,
    pub width: u32,
    pub height: u32,
    pub views: Vec<ViewRun>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FlowRun {
    pub id: String,
    pub steps: u32,
    pub target: u32,
    pub reference: u32,
    pub achieved: bool,
    pub within_target: bool,
    #[serde(default)]
    pub note: String,
}

impl FlowRun {
    pub fn passed(&self) -> bool {
        self.achieved && self.within_target
    }
}

#[derive(Debug, Clone, Default)]
pub struct UiResults {
    pub sizes: Vec<SizeRun>,
    pub flows: Vec<FlowRun>,
}

impl UiResults {
    /// Reads the result files below `root/target/parity`; `None` when the
    /// UI suites have not run.
    pub fn load(root: &Path) -> Option<Self> {
        let dir = root.join(DIR);
        let mut out = UiResults::default();
        for size in ["desktop", "laptop", "kiosk"] {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("views-{size}.json"))) else {
                continue;
            };
            match serde_json::from_str::<SizeRun>(&text) {
                Ok(r) => out.sizes.push(r),
                Err(e) => eprintln!("parity: views-{size}.json unreadable: {e}"),
            }
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("flows")) {
            let mut paths: Vec<_> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            paths.sort();
            for p in paths {
                if let Ok(r) = std::fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str::<FlowRun>(&t).map_err(|e| e.to_string()))
                {
                    out.flows.push(r);
                }
            }
        }
        (!out.sizes.is_empty() || !out.flows.is_empty()).then_some(out)
    }

    /// Distinct required labels found in any view at any size.
    pub fn labels_present(&self) -> BTreeSet<&str> {
        self.sizes
            .iter()
            .flat_map(|s| {
                s.views
                    .iter()
                    .flat_map(|v| v.present.iter().map(String::as_str))
            })
            .collect()
    }

    /// (view, size) pairs that do not pass.
    pub fn failing_views(&self) -> Vec<String> {
        self.sizes
            .iter()
            .flat_map(|s| {
                s.views
                    .iter()
                    .filter(|v| !v.passed)
                    .map(move |v| format!("{} @ {}", v.id, s.size))
            })
            .collect()
    }

    pub fn failing_flows(&self) -> Vec<String> {
        self.flows
            .iter()
            .filter(|f| !f.passed())
            .map(|f| f.id.clone())
            .collect()
    }
}
