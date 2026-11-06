//! Evaluation of rekordbox smart playlist rules (`djmdPlaylist.SmartList`).
//!
//! Smart playlists are imported as static playlists holding the tracks that
//! match at import time. The rule format follows pyrekordbox's
//! `masterdb/smartlist.py`:
//!
//! ```xml
//! <NODE Id="…" LogicalOperator="1" AutomaticUpdate="0">
//!   <CONDITION PropertyName="genre" Operator="8" ValueUnit="" ValueLeft="House" ValueRight=""/>
//! </NODE>
//! ```
//!
//! `LogicalOperator` 1 = all conditions, 2 = any. Operators: 1 equal,
//! 2 not equal, 3 greater, 4 less, 5 in range, 6 in last, 7 not in last,
//! 8 contains, 9 does not contain, 10 starts with, 11 ends with.
//!
//! A playlist with a condition this module cannot evaluate faithfully
//! (My Tag, album artist, mix name, …) is not imported at all, so
//! that no playlist ends up with silently wrong contents.

use crate::model::SrcTrack;
use std::collections::HashMap;

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Attributes of every element named `tag` in document order.
fn elements(xml: &str, tag: &str) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(end) = after.find('>') else { break };
        let body = &after[..end];
        let mut attrs = HashMap::new();
        let mut s = body;
        while let Some(eq) = s.find('=') {
            let name = s[..eq].trim().trim_start_matches('/').trim().to_string();
            let v = s[eq + 1..].trim_start();
            let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(close) = v[1..].find(q) else { break };
            attrs.insert(name, unescape(&v[1..1 + close]));
            s = &v[close + 2..];
        }
        out.push(attrs);
        rest = &after[end..];
    }
    out
}

enum Val<'a> {
    Str(&'a str),
    Num(f64),
    Date(&'a str),
}

fn field<'a>(t: &'a SrcTrack, prop: &str) -> Option<Val<'a>> {
    Some(match prop {
        "artist" => Val::Str(&t.tags.artist),
        "album" => Val::Str(&t.tags.album),
        "originalArtist" => Val::Str(&t.smart.original_artist),
        "comments" => Val::Str(&t.tags.comment),
        "producer" => Val::Str(&t.tags.composer),
        "fileName" => Val::Str(&t.smart.file_name),
        "genre" => Val::Str(&t.tags.genre),
        "key" => Val::Str(&t.smart.key_name),
        "label" => Val::Str(&t.tags.label),
        "remixedBy" => Val::Str(&t.tags.remixer),
        "name" => Val::Str(&t.tags.title),
        "bpm" => Val::Num(t.bpm.unwrap_or(0.0)),
        "counter" => Val::Num(t.smart.play_count as f64),
        "rating" => Val::Num(t.tags.rating as f64),
        "duration" => Val::Num(t.duration),
        "year" => Val::Num(t.tags.year.unwrap_or(0) as f64),
        "grouping" => Val::Num(t.smart.color_id as f64),
        "stockDate" => Val::Date(&t.smart.stock_date),
        "dateCreated" => Val::Date(&t.smart.date_created),
        "dateReleased" => Val::Date(&t.smart.release_date),
        _ => return None,
    })
}

/// Days since 1970-01-01 of a `YYYY-MM-DD…` string.
fn days(s: &str) -> Option<i64> {
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs / 86_400
}

struct Cond {
    prop: String,
    op: u32,
    unit: String,
    left: String,
    right: String,
}

fn num(prop: &str, s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    // rekordbox stores BPM ×100 in the database; rule values may use either.
    Some(if prop == "bpm" && v >= 1000.0 { v / 100.0 } else { v })
}

fn matches(c: &Cond, t: &SrcTrack, now: i64) -> Option<bool> {
    let v = field(t, &c.prop)?;
    Some(match v {
        Val::Str(s) => {
            let (s, l) = (s.to_lowercase(), c.left.to_lowercase());
            match c.op {
                1 => s == l,
                2 => s != l,
                8 => s.contains(&l),
                9 => !s.contains(&l),
                10 => s.starts_with(&l),
                11 => s.ends_with(&l),
                _ => return None,
            }
        }
        Val::Num(x) => {
            let l = num(&c.prop, &c.left)?;
            match c.op {
                1 => (x - l).abs() < 1e-6,
                2 => (x - l).abs() >= 1e-6,
                3 => x > l,
                4 => x < l,
                5 => {
                    let r = num(&c.prop, &c.right)?;
                    x >= l.min(r) && x <= l.max(r)
                }
                _ => return None,
            }
        }
        Val::Date(s) => {
            let Some(x) = days(s) else { return Some(false) };
            match c.op {
                1 => Some(x) == days(&c.left),
                2 => Some(x) != days(&c.left),
                3 => x > days(&c.left)?,
                4 => x < days(&c.left)?,
                5 => x >= days(&c.left)? && x <= days(&c.right)?,
                6 | 7 => {
                    let n: i64 = c.left.trim().parse().ok()?;
                    let span = match c.unit.as_str() {
                        "day" => n,
                        "month" => n * 30,
                        _ => return None,
                    };
                    let inside = x > now - span;
                    if c.op == 6 { inside } else { !inside }
                }
                _ => return None,
            }
        }
    })
}

/// Source ids of the tracks matching the rules, in library order.
pub(crate) fn evaluate(xml: &str, tracks: &[SrcTrack]) -> Result<Vec<String>, String> {
    let node = elements(xml, "NODE").into_iter().next().ok_or("no rules")?;
    let any = node.get("LogicalOperator").map(|s| s.trim() == "2").unwrap_or(false);
    let conds: Vec<Cond> = elements(xml, "CONDITION")
        .into_iter()
        .map(|a| {
            let g = |k: &str| a.get(k).cloned().unwrap_or_default();
            Cond {
                prop: g("PropertyName"),
                op: g("Operator").trim().parse().unwrap_or(0),
                unit: g("ValueUnit"),
                left: g("ValueLeft"),
                right: g("ValueRight"),
            }
        })
        .collect();
    if conds.is_empty() {
        return Err("no rules".into());
    }
    let now = today();
    // Check every condition is supported before evaluating.
    if let Some(t) = tracks.first() {
        for c in &conds {
            if field(t, &c.prop).is_none() {
                return Err(format!("rule on \"{}\" is not supported", c.prop));
            }
        }
    }
    let mut out = Vec::new();
    for t in tracks {
        let mut results = Vec::with_capacity(conds.len());
        for c in &conds {
            match matches(c, t, now) {
                Some(b) => results.push(b),
                None => return Err(format!("rule on \"{}\" with operator {} is not supported", c.prop, c.op)),
            }
        }
        let ok = if any { results.iter().any(|b| *b) } else { results.iter().all(|b| *b) };
        if ok {
            out.push(t.src_id.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_attributes() {
        let e = elements(
            r#"<NODE Id="-1" LogicalOperator="2"><CONDITION PropertyName="genre" Operator="8" ValueLeft="R&amp;B" ValueRight=""/></NODE>"#,
            "CONDITION",
        );
        assert_eq!(e[0]["ValueLeft"], "R&B");
        assert_eq!(e[0]["Operator"], "8");
    }

    #[test]
    fn civil_days() {
        assert_eq!(days("1970-01-01"), Some(0));
        assert_eq!(days("2000-03-01"), Some(11_017));
    }
}
