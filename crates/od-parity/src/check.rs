//! Validation and the gap report.
//!
//! Two kinds of findings:
//!
//! - **errors**: the tables are wrong (schema, duplicate ids, an exclusion
//!   SPEC §1 does not name, a test id that does not exist, a flow whose
//!   target is worse than the usual count). Always enforced by
//!   `features_toml_is_valid`.
//! - **gaps**: the product is not there yet (an in-scope feature not done,
//!   a done feature without a test, a flow without its UI test). Enforced
//!   only in strict mode (`OD_PARITY_STRICT=1` / `parity check --strict`).

use crate::discover::TestIndex;
use crate::labels;
use crate::ui_results::UiResults;
use crate::model::{EXCLUSIONS, FeaturesFile, FlowsFile, Scope, Status, ViewsFile};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

#[derive(Debug, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// In-scope, not deferred, not done: (area, id, status, prio, name).
    pub not_done: Vec<(String, String, Status, String, String)>,
    pub done_without_test: Vec<String>,
    pub unknown_tests: Vec<(String, String)>,
    pub invalid_exclusions: Vec<String>,
    pub deferred: Vec<(String, String)>,
    pub flows_without_test: Vec<String>,
    /// status → count (in scope, not deferred)
    pub status_counts: BTreeMap<Status, usize>,
    /// area → status → count (in scope, not deferred)
    pub area_counts: BTreeMap<String, BTreeMap<Status, usize>>,
    pub area_order: Vec<(String, String)>,
    pub total: usize,
    pub out_of_scope: usize,
    pub tests_known: usize,
    pub tests_linked: usize,
    pub views: usize,
    pub view_labels: usize,
    pub labels_in_ui: usize,
    pub flows: usize,
    /// Headless UI suite results (None = not run).
    pub ui: Option<UiResults>,
    /// Distinct required labels found by the view test.
    pub labels_found: usize,
    /// Failing (view @ size) pairs and flows, from the UI suites.
    pub views_failing: Vec<String>,
    pub flows_failing: Vec<String>,
}

impl Report {
    pub fn in_scope(&self) -> usize {
        self.status_counts.values().sum()
    }

    /// Number of gaps that strict mode fails on.
    pub fn gap_count(&self) -> usize {
        self.not_done.len() + self.done_without_test.len() + self.flows_without_test.len() + self.views_failing.len() + self.flows_failing.len()
    }
}

pub fn is_kebab_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn check(
    features: &FeaturesFile,
    views: &ViewsFile,
    flows: &FlowsFile,
    tests: &TestIndex,
    ui_labels: &BTreeSet<String>,
    ui: Option<&UiResults>,
) -> Report {
    let mut r = Report {
        tests_known: tests.len(),
        ..Report::default()
    };
    for dup in &tests.duplicates {
        r.warnings.push(format!("test id `{dup}` is defined more than once (ambiguous)"));
    }
    check_features(features, tests, &mut r);
    check_views(views, features, ui_labels, &mut r);
    check_flows(flows, views, tests, &mut r);
    if let Some(ui) = ui {
        let required: BTreeSet<&str> = views.views.iter().flat_map(|v| v.regions.iter().flat_map(|r| r.elements.iter().map(String::as_str))).collect();
        r.labels_found = ui.labels_present().intersection(&required).count();
        r.views_failing = ui.failing_views();
        r.flows_failing = ui.failing_flows();
        r.ui = Some(ui.clone());
    }
    r
}

fn check_features(f: &FeaturesFile, tests: &TestIndex, r: &mut Report) {
    let mut areas = BTreeSet::new();
    for a in &f.areas {
        if !is_kebab_id(&a.id) {
            r.errors.push(format!("area id `{}` is not kebab-case", a.id));
        }
        if !areas.insert(a.id.clone()) {
            r.errors.push(format!("area `{}` is defined twice", a.id));
        }
        r.area_order.push((a.id.clone(), a.title.clone()));
    }
    let mut ids = BTreeSet::new();
    let mut linked = BTreeSet::new();
    for feat in &f.features {
        r.total += 1;
        let id = &feat.id;
        if !is_kebab_id(id) {
            r.errors.push(format!("feature id `{id}` is not kebab-case"));
        }
        if !ids.insert(id.clone()) {
            r.errors.push(format!("feature id `{id}` is used twice"));
        }
        if !areas.contains(&feat.area) {
            r.errors.push(format!("feature `{id}`: unknown area `{}`", feat.area));
        }
        if feat.name.trim().is_empty() {
            r.errors.push(format!("feature `{id}`: empty name"));
        }
        if let Some(p) = &feat.prio {
            if !["P1", "P2", "P3"].contains(&p.as_str()) {
                r.errors.push(format!("feature `{id}`: prio `{p}` is not P1, P2 or P3"));
            }
        }
        if let Some(m) = &feat.milestone {
            if !m.starts_with('M') || m[1..].parse::<u32>().is_err() {
                r.errors.push(format!("feature `{id}`: milestone `{m}` is not M<n>"));
            }
        }
        for t in &feat.tests {
            linked.insert(t.clone());
            match tests.get(t) {
                None => {
                    r.errors.push(format!("feature `{id}`: test `{t}` does not exist"));
                    r.unknown_tests.push((id.clone(), t.clone()));
                }
                Some(found) if found.ignored => r.warnings.push(format!(
                    "feature `{id}`: test `{t}` is #[ignore]d and does not run by default"
                )),
                Some(_) => {}
            }
        }
        match feat.scope {
            Scope::Out => {
                r.out_of_scope += 1;
                match &feat.exclusion {
                    Some(e) if EXCLUSIONS.contains(&e.as_str()) => {}
                    Some(e) => {
                        r.errors.push(format!(
                            "feature `{id}`: exclusion `{e}` is not named in SPEC §1 (allowed: {})",
                            EXCLUSIONS.join(", ")
                        ));
                        r.invalid_exclusions.push(id.clone());
                    }
                    None => {
                        r.errors.push(format!("feature `{id}`: scope = \"out\" needs an exclusion"));
                        r.invalid_exclusions.push(id.clone());
                    }
                }
                if feat.milestone.is_some() {
                    r.errors.push(format!(
                        "feature `{id}`: out-of-scope rows take no milestone (use scope = \"in\" + milestone)"
                    ));
                }
            }
            Scope::In => {
                if feat.exclusion.is_some() {
                    r.errors.push(format!("feature `{id}`: in-scope rows take no exclusion"));
                    r.invalid_exclusions.push(id.clone());
                }
                if feat.status.is_none() {
                    r.errors.push(format!("feature `{id}`: in-scope rows need a status"));
                }
                if feat.is_deferred(&f.meta) {
                    r.deferred.push((
                        id.clone(),
                        format!("{} ({}, {})", feat.name, feat.status(), feat.milestone.as_deref().unwrap_or("")),
                    ));
                    continue;
                }
                let status = feat.status();
                *r.status_counts.entry(status).or_default() += 1;
                *r.area_counts
                    .entry(feat.area.clone())
                    .or_default()
                    .entry(status)
                    .or_default() += 1;
                if status == Status::Done {
                    if feat.tests.is_empty() {
                        r.done_without_test.push(id.clone());
                    }
                } else {
                    r.not_done.push((
                        feat.area.clone(),
                        id.clone(),
                        status,
                        feat.prio.clone().unwrap_or_default(),
                        feat.name.clone(),
                    ));
                }
            }
        }
    }
    r.tests_linked = linked.len();
}

fn check_views(v: &ViewsFile, f: &FeaturesFile, ui_labels: &BTreeSet<String>, r: &mut Report) {
    let feature_ids: BTreeSet<&str> = f.features.iter().map(|x| x.id.as_str()).collect();
    let sizes: BTreeSet<&str> = v.sizes.iter().map(|s| s.id.as_str()).collect();
    if sizes.len() != v.sizes.len() {
        r.errors.push("views.toml: duplicate size id".into());
    }
    for need in [(1440, 900), (1280, 800), (800, 480)] {
        if !v.sizes.iter().any(|s| (s.width, s.height) == need) {
            r.errors.push(format!("views.toml: required size {}x{} missing", need.0, need.1));
        }
    }
    let mut inv = BTreeSet::new();
    for i in &v.invariants {
        if !inv.insert(i.id.as_str()) {
            r.errors.push(format!("views.toml: invariant `{}` defined twice", i.id));
        }
    }
    let mut view_ids = BTreeSet::new();
    let mut all_labels = BTreeSet::new();
    for view in &v.views {
        r.views += 1;
        if !view_ids.insert(view.id.as_str()) {
            r.errors.push(format!("view `{}` defined twice", view.id));
        }
        if !view.id.split('.').all(is_kebab_id) {
            r.errors.push(format!("view id `{}` must be dotted kebab-case", view.id));
        }
        for s in &view.sizes {
            if !sizes.contains(s.as_str()) {
                r.errors.push(format!("view `{}`: unknown size `{s}`", view.id));
            }
        }
        for fid in &view.features {
            if !feature_ids.contains(fid.as_str()) {
                r.errors.push(format!("view `{}`: unknown feature `{fid}`", view.id));
            }
        }
        if view.regions.is_empty() {
            r.errors.push(format!("view `{}` has no regions", view.id));
        }
        let bands: BTreeSet<&str> = view.bands.iter().map(String::as_str).collect();
        if bands.len() != view.bands.len() || bands.is_empty() {
            r.errors.push(format!("view `{}`: bands must be non-empty and unique", view.id));
        }
        let mut in_view = BTreeSet::new();
        let mut region_ids = BTreeSet::new();
        for region in &view.regions {
            if !region_ids.insert(region.id.as_str()) {
                r.errors.push(format!("view `{}`: region `{}` twice", view.id, region.id));
            }
            if !bands.contains(region.band.as_str()) {
                r.errors.push(format!(
                    "view `{}` region `{}`: band `{}` is not in the view's bands",
                    view.id, region.id, region.band
                ));
            }
            if !["row", "column", "grid", "any"].contains(&region.order.as_str()) {
                r.errors.push(format!(
                    "view `{}` region `{}`: order `{}` is not row|column|grid|any",
                    view.id, region.id, region.order
                ));
            }
            if region.elements.is_empty() {
                r.errors.push(format!("view `{}` region `{}` has no elements", view.id, region.id));
            }
            for label in &region.elements {
                if !labels::is_valid_label(label) {
                    r.errors.push(format!("view `{}`: label `{label}` breaks the label convention", view.id));
                }
                if !in_view.insert(label.as_str()) {
                    r.errors.push(format!("view `{}`: label `{label}` listed twice", view.id));
                }
                all_labels.insert(label.clone());
            }
        }
        // Kiosk exemptions: only for views shown at the kiosk size, only
        // labels of the view, each group with a reason, never what the
        // controller cannot do.
        for k in &view.kiosk_omit {
            if !view.sizes.is_empty() && !view.sizes.iter().any(|s| s == crate::model::KIOSK_SIZE) {
                r.errors.push(format!("view `{}`: kiosk exemptions but the view is not shown at the kiosk size", view.id));
            }
            if k.reason.trim().is_empty() || k.omit.is_empty() {
                r.errors.push(format!("view `{}`: a kiosk exemption needs labels and a reason", view.id));
            }
            for label in &k.omit {
                if !in_view.contains(label.as_str()) {
                    r.errors.push(format!("view `{}`: kiosk exemption `{label}` is not a required element", view.id));
                }
                let part = label.split_once('.').map(|(_, p)| p).unwrap_or(label);
                let never = crate::model::KIOSK_NEVER_OMIT.contains(&part) || crate::model::KIOSK_NEVER_OMIT_LABELS.contains(&label.as_str());
                if never && k.shown_in.is_empty() {
                    r.errors.push(format!("view `{}`: `{label}` must stay on the kiosk screen (the controller cannot show or do it)", view.id));
                }
            }
            // Shown on another kiosk screen: that view must require them at
            // the kiosk size.
            if !k.shown_in.is_empty() {
                match v.views.iter().find(|o| o.id == k.shown_in) {
                    None => r.errors.push(format!("view `{}`: kiosk `shown_in` names an unknown view `{}`", view.id, k.shown_in)),
                    Some(o) => {
                        let at_kiosk = o.sizes.is_empty() || o.sizes.iter().any(|s| s == crate::model::KIOSK_SIZE);
                        for label in &k.omit {
                            let there = o.regions.iter().any(|r| r.elements.contains(label)) && !o.omitted_at(crate::model::KIOSK_SIZE, label);
                            if !at_kiosk || !there {
                                r.errors.push(format!("view `{}`: `{label}` is not checked at the kiosk size in `{}`", view.id, k.shown_in));
                            }
                        }
                    }
                }
            }
        }
    }
    r.view_labels = all_labels.len();
    r.labels_in_ui = all_labels.intersection(ui_labels).count();
}

fn check_flows(fl: &FlowsFile, v: &ViewsFile, tests: &TestIndex, r: &mut Report) {
    let labels: BTreeSet<&str> = v
        .views
        .iter()
        .flat_map(|v| v.regions.iter().flat_map(|r| r.elements.iter().map(String::as_str)))
        .collect();
    let mut ids = BTreeSet::new();
    for flow in &fl.flows {
        r.flows += 1;
        if !is_kebab_id(&flow.id) || !ids.insert(flow.id.as_str()) {
            r.errors.push(format!("flow id `{}` is not kebab-case or not unique", flow.id));
        }
        if flow.target_steps > flow.reference_steps {
            r.errors.push(format!(
                "flow `{}`: target {} steps is worse than the usual {}",
                flow.id, flow.target_steps, flow.reference_steps
            ));
        }
        if flow.reference_path.len() as u32 != flow.reference_steps {
            r.errors.push(format!(
                "flow `{}`: reference_path has {} steps, reference_steps says {}",
                flow.id,
                flow.reference_path.len(),
                flow.reference_steps
            ));
        }
        if flow.opendecks_path.len() as u32 != flow.target_steps {
            r.errors.push(format!(
                "flow `{}`: opendecks_path has {} steps, target_steps says {}",
                flow.id,
                flow.opendecks_path.len(),
                flow.target_steps
            ));
        }
        if flow.reference_note.trim().is_empty() {
            r.errors.push(format!("flow `{}`: reference_note must say how the usual step count was established", flow.id));
        }
        for l in &flow.labels {
            if !labels.contains(l.as_str()) {
                r.errors.push(format!("flow `{}`: label `{l}` is not in views.toml", flow.id));
            }
        }
        match &flow.ui_test {
            Some(t) if tests.get(t).is_none() => {
                r.errors.push(format!("flow `{}`: ui_test `{t}` does not exist", flow.id));
                r.unknown_tests.push((flow.id.clone(), t.clone()));
            }
            Some(_) => {}
            None => r.flows_without_test.push(flow.id.clone()),
        }
    }
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 {
        "0".into()
    } else {
        format!("{:.0}", 100.0 * a as f64 / b as f64)
    }
}

/// The gap report as Markdown (printed by `parity check`, uploaded by CI).
pub fn render(r: &Report, strict: bool) -> String {
    let mut s = String::new();
    let done = r.status_counts.get(&Status::Done).copied().unwrap_or(0);
    let _ = writeln!(s, "# Parity gap report (SPEC §16)\n");
    let _ = writeln!(
        s,
        "{} features: {} in scope ({} DONE = {} %), {} deferred, {} out of scope. \
         {} tests found in the workspace, {} linked.\n",
        r.total,
        r.in_scope(),
        done,
        pct(done, r.in_scope()),
        r.deferred.len(),
        r.out_of_scope,
        r.tests_known,
        r.tests_linked
    );
    let _ = writeln!(
        s,
        "Result: {} errors, {} gaps{}.\n",
        r.errors.len(),
        r.gap_count(),
        if strict { " (strict mode: gaps fail)" } else { " (gaps do not fail yet; OD_PARITY_STRICT=1 makes them fail)" }
    );

    let _ = writeln!(s, "## Status per area (in scope, not deferred)\n");
    let _ = write!(s, "| Area |");
    for st in Status::ALL {
        let _ = write!(s, " {st} |");
    }
    let _ = writeln!(s, " Total |");
    let _ = writeln!(s, "|---|---:|---:|---:|---:|---:|");
    for (id, title) in &r.area_order {
        let Some(counts) = r.area_counts.get(id) else { continue };
        let _ = write!(s, "| {title} |");
        for st in Status::ALL {
            let _ = write!(s, " {} |", counts.get(&st).copied().unwrap_or(0));
        }
        let _ = writeln!(s, " {} |", counts.values().sum::<usize>());
    }
    let _ = write!(s, "| **Total** |");
    for st in Status::ALL {
        let _ = write!(s, " **{}** |", r.status_counts.get(&st).copied().unwrap_or(0));
    }
    let _ = writeln!(s, " **{}** |\n", r.in_scope());

    if !r.errors.is_empty() {
        let _ = writeln!(s, "## Errors ({})\n", r.errors.len());
        for e in &r.errors {
            let _ = writeln!(s, "- {e}");
        }
        let _ = writeln!(s);
    }

    let _ = writeln!(s, "## In-scope features not done ({})\n", r.not_done.len());
    for st in [Status::Partial, Status::EngineOnly, Status::Missing] {
        let rows: Vec<_> = r.not_done.iter().filter(|x| x.2 == st).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(s, "### {st} ({})\n", rows.len());
        for (area, id, _, prio, name) in rows {
            let p = if prio.is_empty() { String::new() } else { format!(" [{prio}]") };
            let _ = writeln!(s, "- `{id}`{p} ({area}): {name}");
        }
        let _ = writeln!(s);
    }

    let _ = writeln!(s, "## DONE without a test ({})\n", r.done_without_test.len());
    for id in &r.done_without_test {
        let _ = writeln!(s, "- `{id}`");
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Unknown test ids ({})\n", r.unknown_tests.len());
    for (owner, t) in &r.unknown_tests {
        let _ = writeln!(s, "- `{owner}` → `{t}`");
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Invalid exclusions ({})\n", r.invalid_exclusions.len());
    for id in &r.invalid_exclusions {
        let _ = writeln!(s, "- `{id}`");
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Deferred (tracked, not counted) ({})\n", r.deferred.len());
    for (id, what) in &r.deferred {
        let _ = writeln!(s, "- `{id}`: {what}");
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "## Views and flows\n");
    let _ = writeln!(
        s,
        "- {} views, {} distinct required labels; {} of them ({} %) appear literally as \
         `accessible-label` in `crates/od-ui` (static scan; the headless view test is the gate).",
        r.views,
        r.view_labels,
        r.labels_in_ui,
        pct(r.labels_in_ui, r.view_labels)
    );
    let _ = writeln!(
        s,
        "- {} superiority flows, {} without a driving UI test{}",
        r.flows,
        r.flows_without_test.len(),
        if r.flows_without_test.is_empty() {
            ".".to_owned()
        } else {
            format!(": {}", r.flows_without_test.iter().map(|f| format!("`{f}`")).collect::<Vec<_>>().join(", "))
        }
    );
    render_ui(r, &mut s);
    if !r.warnings.is_empty() {
        let _ = writeln!(s, "\n## Warnings ({})\n", r.warnings.len());
        for w in &r.warnings {
            let _ = writeln!(s, "- {w}");
        }
    }
    s
}

/// The headless UI suite results (view completeness, flows).
fn render_ui(r: &Report, s: &mut String) {
    let _ = writeln!(s, "\n## Headless UI suites (SPEC §16.3, §16.4)\n");
    let Some(ui) = &r.ui else {
        let _ = writeln!(s, "Not run: `cargo test -p od-ui ui_tests` writes `target/parity/` (views and flows).");
        return;
    };
    let _ = writeln!(
        s,
        "- Labels present: {} of {} required labels ({} %) found on screen by the view test.",
        r.labels_found,
        r.view_labels,
        pct(r.labels_found, r.view_labels)
    );
    for size in &ui.sizes {
        let pass = size.views.iter().filter(|v| v.passed).count();
        let avail = size.views.iter().filter(|v| v.available).count();
        let _ = writeln!(
            s,
            "- Views at {} ({}×{}): {} of {} pass; {} of them can be opened in the app.",
            size.size,
            size.width,
            size.height,
            pass,
            size.views.len(),
            avail
        );
    }
    if !ui.sizes.is_empty() {
        let _ = writeln!(s, "\n| View | {} |", ui.sizes.iter().map(|x| x.size.as_str()).collect::<Vec<_>>().join(" | "));
        let _ = writeln!(s, "|---|{}", ui.sizes.iter().map(|_| "---|").collect::<String>());
        let mut ids: Vec<&str> = Vec::new();
        for size in &ui.sizes {
            for v in &size.views {
                if !ids.contains(&v.id.as_str()) {
                    ids.push(&v.id);
                }
            }
        }
        for id in ids {
            let cells: Vec<String> = ui
                .sizes
                .iter()
                .map(|size| match size.views.iter().find(|v| v.id == id) {
                    None => "—".into(),
                    Some(v) if v.passed => "PASS".into(),
                    Some(v) if !v.available => "n/a".into(),
                    Some(v) => format!("{}/{} ({} issues)", v.present.len(), v.required, v.failures.len()),
                })
                .collect();
            let _ = writeln!(s, "| `{id}` | {} |", cells.join(" | "));
        }
    }
    if !ui.flows.is_empty() {
        let _ = writeln!(s, "\n| Flow | Usual | Target | Measured | Result |\n|---|---:|---:|---:|---|");
        for f in &ui.flows {
            let res = if f.passed() { "PASS".to_string() } else if !f.achieved { format!("not achievable: {}", f.note) } else { format!("over target: {}", f.note) };
            let _ = writeln!(s, "| `{}` | {} | {} | {} | {res} |", f.id, f.reference, f.target, f.steps);
        }
    }
}
