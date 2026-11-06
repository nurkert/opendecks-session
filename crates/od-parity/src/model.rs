//! The three parity tables under `tests/parity/` (SPEC §16).

use serde::Deserialize;
use std::fmt;
use std::path::Path;

/// Exclusions that SPEC §1 names. Nothing else may be out of scope.
pub const EXCLUSIONS: &[&str] = &["streaming", "cloud", "video", "lighting", "dvs"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Done,
    Partial,
    EngineOnly,
    Missing,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::Done, Status::Partial, Status::EngineOnly, Status::Missing];

    pub fn label(self) -> &'static str {
        match self {
            Status::Done => "DONE",
            Status::Partial => "PARTIAL",
            Status::EngineOnly => "ENGINE-ONLY",
            Status::Missing => "MISSING",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    In,
    Out,
}

/// `tests/parity/features.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesFile {
    pub meta: FeaturesMeta,
    #[serde(rename = "area")]
    pub areas: Vec<Area>,
    #[serde(rename = "feature")]
    pub features: Vec<Feature>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesMeta {
    /// Snapshot date of the inventory (shown in the generated matrix).
    pub snapshot: String,
    /// Milestones whose features are tracked but not counted as gaps.
    #[serde(default)]
    pub deferred_milestones: Vec<String>,
    /// Markdown placed between the title and the legend of the matrix.
    #[serde(default)]
    pub intro: String,
    /// Markdown appended after the counts (implementation notes).
    #[serde(default)]
    pub appendix: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub area: String,
    pub name: String,
    pub scope: Scope,
    /// For `scope = "out"`: one of [`EXCLUSIONS`].
    #[serde(default)]
    pub exclusion: Option<String>,
    /// For out-of-scope rows without an implementation: may be omitted.
    #[serde(default)]
    pub status: Option<Status>,
    /// Shown after the status, e.g. "beyond rekordbox" or "own".
    #[serde(default)]
    pub qualifier: Option<String>,
    /// A milestone such as "M6": deferred when listed in
    /// `meta.deferred_milestones`.
    #[serde(default)]
    pub milestone: Option<String>,
    #[serde(default)]
    pub prio: Option<String>,
    /// Test ids (see [`crate::discover`]) that prove the feature.
    #[serde(default)]
    pub tests: Vec<String>,
    /// `file::function` references into the code.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Free text: what is missing, why it is excluded, …
    #[serde(default)]
    pub note: String,
    /// Where the feature is required (SPEC section, UX study section).
    #[serde(default)]
    pub spec: Option<String>,
}

impl Feature {
    pub fn status(&self) -> Status {
        self.status.unwrap_or(Status::Missing)
    }
    pub fn is_deferred(&self, meta: &FeaturesMeta) -> bool {
        self.milestone
            .as_ref()
            .is_some_and(|m| meta.deferred_milestones.contains(m))
    }
}

/// `tests/parity/views.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewsFile {
    pub meta: ViewsMeta,
    #[serde(rename = "size")]
    pub sizes: Vec<Size>,
    #[serde(rename = "invariant")]
    pub invariants: Vec<Invariant>,
    #[serde(rename = "view")]
    pub views: Vec<View>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewsMeta {
    pub source: String,
    /// Regex-like description of the label grammar (documentation only;
    /// the checker uses [`crate::labels::is_valid_label`]).
    pub label_grammar: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub id: String,
    pub width: u32,
    pub height: u32,
    /// Minimum touch target edge in px.
    pub min_target: u32,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invariant {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub tolerance_px: Option<u32>,
    #[serde(default)]
    pub min_ratio: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    pub title: String,
    /// prepare | perform | preferences | menu
    pub mode: String,
    /// Line or section in the UX study.
    pub source: String,
    /// Size ids; empty = all sizes.
    #[serde(default)]
    pub sizes: Vec<String>,
    /// Top-level bands from top to bottom (UX study §1.1: fixed-height
    /// bands, the browser takes the rest). Every region names one.
    pub bands: Vec<String>,
    /// Ids of features this view exposes (optional cross-reference).
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub note: String,
    #[serde(rename = "region")]
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: String,
    /// The band (from [`View::bands`]) the region lies in.
    pub band: String,
    /// Where in the band, as the UX study describes it ("left, 46 %",
    /// "mirrors deck 1"). Documentation for the view test author.
    #[serde(default)]
    pub place: String,
    /// Direction in which `elements` are ordered: "row" (left to right),
    /// "column" (top to bottom), "grid" (row-major) or "any".
    pub order: String,
    /// Required accessible labels in their required order.
    pub elements: Vec<String>,
    #[serde(default)]
    pub note: String,
}

/// `tests/parity/flows.toml`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowsFile {
    pub meta: FlowsMeta,
    #[serde(rename = "flow")]
    pub flows: Vec<Flow>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowsMeta {
    pub step_rule: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub task: String,
    pub rekordbox_steps: u32,
    /// File and line(s) of the UX study the count comes from.
    pub rekordbox_source: String,
    pub rekordbox_path: Vec<String>,
    /// True when the rekordbox path was inferred, not observed.
    #[serde(default)]
    pub inferred: bool,
    pub target_steps: u32,
    pub opendecks_path: Vec<String>,
    /// UI test driving the flow (empty until it exists).
    #[serde(default)]
    pub ui_test: Option<String>,
    /// Labels the flow touches (must exist in views.toml).
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub note: String,
}

pub fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
