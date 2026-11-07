//! Smart playlist rules, evaluated against track records.

use crate::TrackRecord;
use od_core::MusicalKey;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Title,
    Artist,
    Album,
    Genre,
    Label,
    Comment,
    Bpm,
    Key,
    Rating,
    Year,
    /// Days since the track was added.
    AddedDaysAgo,
    PlayCount,
    /// Seconds.
    Duration,
    Color,
    Format,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Contains,
    NotContains,
    Is,
    IsNot,
    GreaterThan,
    LessThan,
    /// Inclusive range; value is `[low, high]`.
    Between,
    /// Key within one step on the Camelot wheel (value: a key like "8A").
    HarmonicWith,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    All(Vec<Rule>),
    Any(Vec<Rule>),
    Cond {
        field: Field,
        op: Op,
        value: serde_json::Value,
    },
}

enum FieldValue<'a> {
    Text(&'a str),
    Owned(String),
    Number(Option<f64>),
}

fn field_value(t: &TrackRecord, f: Field) -> FieldValue<'_> {
    match f {
        Field::Title => FieldValue::Text(&t.tags.title),
        Field::Artist => FieldValue::Text(&t.tags.artist),
        Field::Album => FieldValue::Text(&t.tags.album),
        Field::Genre => FieldValue::Text(&t.tags.genre),
        Field::Label => FieldValue::Text(&t.tags.label),
        Field::Comment => FieldValue::Text(&t.tags.comment),
        Field::Bpm => FieldValue::Number(t.bpm),
        Field::Key => FieldValue::Owned(t.key.map(|k| k.to_string()).unwrap_or_default()),
        Field::Rating => FieldValue::Number(Some(t.tags.rating as f64)),
        Field::Year => FieldValue::Number(t.tags.year.map(f64::from)),
        Field::AddedDaysAgo => FieldValue::Number(days_ago(&t.date_added)),
        Field::PlayCount => FieldValue::Number(Some(t.play_count as f64)),
        Field::Duration => FieldValue::Number(Some(t.duration)),
        Field::Color => FieldValue::Owned(t.tags.color.map(|c| c.to_hex()).unwrap_or_default()),
        Field::Format => {
            FieldValue::Owned(t.format.map(|f| f.as_str().to_string()).unwrap_or_default())
        }
    }
}

/// Days between an ISO timestamp (`YYYY-MM-DDTHH:MM:SSZ`) and now.
fn days_ago(iso: &str) -> Option<f64> {
    let date = iso.get(..10)?;
    let mut parts = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    let then = days_from_civil(y, m, d);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64
        / 86_400;
    Some((now - then) as f64)
}

/// Days since 1970-01-01 for a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn as_number(v: &serde_json::Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

impl Rule {
    pub fn matches(&self, t: &TrackRecord) -> bool {
        match self {
            Rule::All(rules) => rules.iter().all(|r| r.matches(t)),
            Rule::Any(rules) => rules.is_empty() || rules.iter().any(|r| r.matches(t)),
            Rule::Cond { field, op, value } => cond_matches(t, *field, *op, value),
        }
    }
}

fn cond_matches(t: &TrackRecord, field: Field, op: Op, value: &serde_json::Value) -> bool {
    if op == Op::HarmonicWith {
        let (Some(k), Some(target)) = (t.key, value.as_str().and_then(MusicalKey::parse)) else {
            return false;
        };
        return target.compatible().contains(&k);
    }
    match field_value(t, field) {
        FieldValue::Text(s) => text_matches(s, op, value),
        FieldValue::Owned(s) => text_matches(&s, op, value),
        FieldValue::Number(n) => {
            let Some(n) = n else { return false };
            match op {
                Op::Is => as_number(value).is_some_and(|v| (n - v).abs() < 1e-6),
                Op::IsNot => as_number(value).is_none_or(|v| (n - v).abs() >= 1e-6),
                Op::GreaterThan => as_number(value).is_some_and(|v| n > v),
                Op::LessThan => as_number(value).is_some_and(|v| n < v),
                Op::Between => match value
                    .as_array()
                    .map(|a| a.iter().filter_map(as_number).collect::<Vec<_>>())
                {
                    Some(r) if r.len() == 2 => n >= r[0].min(r[1]) && n <= r[0].max(r[1]),
                    _ => false,
                },
                _ => false,
            }
        }
    }
}

fn text_matches(s: &str, op: Op, value: &serde_json::Value) -> bool {
    let v = value
        .as_str()
        .map(str::to_lowercase)
        .unwrap_or_else(|| value.to_string());
    let s = s.to_lowercase();
    match op {
        Op::Contains => s.contains(&v),
        Op::NotContains => !s.contains(&v),
        Op::Is => s == v,
        Op::IsNot => s != v,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::track;
    use crate::{Library, NodeKind};
    use serde_json::json;

    #[test]
    fn smart_playlist_filters() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "House One", Some(124.0), Some("8A"));
        let _b = track(&mut lib, "Techno Two", Some(132.0), Some("8A"));
        let c = track(&mut lib, "House Three", Some(122.0), Some("9A"));
        let _d = track(&mut lib, "House Four", Some(123.0), Some("2B"));
        let smart = lib
            .create_node(None, NodeKind::Smart, "Warm house")
            .unwrap();
        lib.set_smart_rule(
            smart,
            Rule::All(vec![
                Rule::Cond {
                    field: Field::Title,
                    op: Op::Contains,
                    value: json!("house"),
                },
                Rule::Cond {
                    field: Field::Bpm,
                    op: Op::Between,
                    value: json!([120, 126]),
                },
                Rule::Cond {
                    field: Field::Key,
                    op: Op::HarmonicWith,
                    value: json!("8A"),
                },
            ]),
        )
        .unwrap();
        let mut ids: Vec<_> = lib
            .node_tracks(smart)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec![a, c]);
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert!(days_ago("2020-01-01T00:00:00Z").unwrap() > 1000.0);
    }
}
