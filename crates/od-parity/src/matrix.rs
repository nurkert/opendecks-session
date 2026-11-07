//! Renders `docs/feature-matrix.md` from `tests/parity/features.toml`.

use crate::model::{Feature, FeaturesFile, Scope, Status};
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub const HEADER: &str = "<!-- Generated from tests/parity/features.toml by \
`cargo run -p od-parity --bin parity -- matrix`. Do not edit by hand. -->";

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn status_cell(f: &Feature, file: &FeaturesFile) -> String {
    if f.scope == Scope::Out {
        return "OUT-OF-SCOPE".into();
    }
    let mut s = f.status().label().to_owned();
    if let Some(q) = &f.qualifier {
        let _ = write!(s, " ({q})");
    }
    if let Some(m) = &f.milestone {
        if f.is_deferred(&file.meta) {
            let _ = write!(s, ", deferred to {m}");
        } else {
            let _ = write!(s, ", {m}");
        }
    }
    s
}

fn evidence_cell(f: &Feature) -> String {
    let mut parts = Vec::new();
    // References the note already spells out (in any split form) are not repeated.
    let note = f.note.as_str();
    let extra: Vec<String> = f
        .evidence
        .iter()
        .filter(|e| !e.split("::").all(|part| note.contains(part)))
        .map(|e| format!("`{e}`"))
        .collect();
    if !extra.is_empty() {
        parts.push(extra.join(", "));
    }
    if !f.note.trim().is_empty() {
        parts.push(f.note.trim().to_owned());
    }
    if let Some(spec) = &f.spec {
        parts.push(format!("({spec})"));
    }
    if parts.is_empty() {
        "—".into()
    } else {
        cell(&parts.join("; "))
    }
}

fn prio_cell(f: &Feature) -> String {
    match f.prio.as_deref() {
        Some("P1") => "**P1**".into(),
        Some(p) => p.to_owned(),
        None => String::new(),
    }
}

/// Counts used for the summary line and the counts table.
pub struct Counts {
    pub by_status: BTreeMap<Status, usize>,
    pub deferred: usize,
    pub out: usize,
    pub total: usize,
}

pub fn counts(file: &FeaturesFile) -> Counts {
    let mut c = Counts {
        by_status: BTreeMap::new(),
        deferred: 0,
        out: 0,
        total: file.features.len(),
    };
    for f in &file.features {
        if f.scope == Scope::Out {
            c.out += 1;
        } else if f.is_deferred(&file.meta) {
            c.deferred += 1;
        } else {
            *c.by_status.entry(f.status()).or_default() += 1;
        }
    }
    c
}

pub fn summary_line(file: &FeaturesFile) -> String {
    let c = counts(file);
    let in_scope: usize = c.by_status.values().sum();
    let get = |s| c.by_status.get(&s).copied().unwrap_or(0);
    let done = get(Status::Done);
    let pct = if in_scope == 0 {
        0.0
    } else {
        100.0 * done as f64 / in_scope as f64
    };
    format!(
        "**Summary:** {} features; {} in scope: {} DONE ({:.0} %), {} PARTIAL, {} ENGINE-ONLY, {} MISSING; \
         {} deferred to a later milestone; {} out of scope (SPEC §1).",
        c.total,
        in_scope,
        done,
        pct,
        get(Status::Partial),
        get(Status::EngineOnly),
        get(Status::Missing),
        c.deferred,
        c.out
    )
}

pub fn render(file: &FeaturesFile) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{HEADER}\n");
    let _ = writeln!(s, "# Feature matrix (OpenDecks Session)\n");
    let _ = writeln!(s, "Snapshot: {}.\n", file.meta.snapshot);
    let _ = writeln!(s, "{}\n", summary_line(file));
    if !file.meta.intro.trim().is_empty() {
        let _ = writeln!(s, "{}\n", file.meta.intro.trim());
    }
    let _ = writeln!(s, "## Legend\n");
    let _ = writeln!(s, "| Status | Meaning |\n|---|---|");
    let _ = writeln!(
        s,
        "| DONE | Works end to end, including UI and/or controller |"
    );
    let _ = writeln!(
        s,
        "| ENGINE-ONLY | Backend (engine, library, format) exists; no UI and no controller path |"
    );
    let _ = writeln!(
        s,
        "| PARTIAL | Usable, but an expected part is missing (noted) |"
    );
    let _ = writeln!(s, "| MISSING | Nothing in the code |");
    let _ = writeln!(
        s,
        "| OUT-OF-SCOPE | Excluded by SPEC §1 (streaming, cloud, video, lighting, DVS) |"
    );
    let _ = writeln!(
        s,
        "\nA milestone after the status (\"deferred to M6\") marks an in-scope feature that \
         SPEC §15 schedules later; it is tracked but not counted as a gap. The Tests column \
         counts the automated tests linked in `features.toml` (SPEC §16.1).\n"
    );
    let _ = writeln!(
        s,
        "Priority (gaps only): **P1** = an experienced DJ hits it in the first session and the app \
         feels broken or foreign without it; **P2** = expected, regular use; **P3** = niche, \
         power-user or later.\n"
    );
    let _ = writeln!(s, "---");
    for (n, area) in file.areas.iter().enumerate() {
        let rows: Vec<&Feature> = file.features.iter().filter(|f| f.area == area.id).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = writeln!(s, "\n## {}. {}\n", n + 1, area.title);
        let all_out = rows.iter().all(|f| f.scope == Scope::Out);
        if all_out {
            let _ = writeln!(s, "| Feature | Status | Reason |\n|---|---|---|");
            for f in rows {
                let reason = format!(
                    "SPEC §1: {}{}",
                    f.exclusion.as_deref().unwrap_or("?"),
                    if f.note.trim().is_empty() {
                        String::new()
                    } else {
                        format!("; {}", f.note.trim())
                    }
                );
                let _ = writeln!(
                    s,
                    "| {} | {} | {} |",
                    cell(&f.name),
                    status_cell(f, file),
                    cell(&reason)
                );
            }
        } else {
            let _ = writeln!(
                s,
                "| Feature | Status | Evidence / note | Tests | Prio |\n|---|---|---|---|---|"
            );
            for f in rows {
                let tests = if f.tests.is_empty() {
                    "—".to_owned()
                } else {
                    f.tests.len().to_string()
                };
                let _ = writeln!(
                    s,
                    "| {} | {} | {} | {} | {} |",
                    cell(&f.name),
                    status_cell(f, file),
                    evidence_cell(f),
                    tests,
                    prio_cell(f)
                );
            }
        }
    }
    let c = counts(file);
    let _ = writeln!(s, "\n---\n\n## Counts\n");
    let _ = writeln!(s, "| Status | Rows |\n|---|---|");
    for st in Status::ALL {
        let _ = writeln!(
            s,
            "| {} | {} |",
            st,
            c.by_status.get(&st).copied().unwrap_or(0)
        );
    }
    let _ = writeln!(
        s,
        "| deferred (in scope, later milestone) | {} |",
        c.deferred
    );
    let _ = writeln!(s, "| OUT-OF-SCOPE | {} |", c.out);
    let _ = writeln!(s, "| **Total** | **{}** |", c.total);
    if !file.meta.appendix.trim().is_empty() {
        let _ = writeln!(s, "\n{}", file.meta.appendix.trim());
    }
    s
}
