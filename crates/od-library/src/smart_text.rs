//! A compact text syntax for smart playlist rules, shared by the app's rule
//! editor and the MCP server.
//!
//! Comma-separated conditions, all of which must match:
//!
//! ```text
//! bpm 120-126, key 8A~, genre house, -genre techno, rating >= 3, added < 30d
//! ```
//!
//! * `bpm`, `rating`, `year`, `plays`, `duration` take `N`, `A-B`, `>N`,
//!   `>=N`, `<N`, `<=N`.
//! * `key 8A` matches the key exactly, `key 8A~` harmonically (±1, relative).
//! * Text fields (`title`, `artist`, `album`, `genre`, `label`, `comment`,
//!   `format`) match as "contains"; a leading `-` negates.
//! * `added < 30d` / `added > 1y` compare the age in days (d, w, m, y).
use crate::smart::{Field, Op, Rule};
use serde_json::{Value, json};

fn parse_number_expr(s: &str) -> Option<(Op, Value)> {
    let s = s.trim();
    if let Some((a, b)) = s.split_once('-').filter(|(a, _)| !a.trim().is_empty()) {
        let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        return Some((Op::Between, json!([a, b])));
    }
    let (op, rest, adjust) = if let Some(r) = s.strip_prefix(">=") {
        (Op::GreaterThan, r, -1e-9)
    } else if let Some(r) = s.strip_prefix("<=") {
        (Op::LessThan, r, 1e-9)
    } else if let Some(r) = s.strip_prefix('>') {
        (Op::GreaterThan, r, 0.0)
    } else if let Some(r) = s.strip_prefix('<') {
        (Op::LessThan, r, 0.0)
    } else {
        (Op::Is, s, 0.0)
    };
    let v: f64 = rest.trim().parse().ok()?;
    Some((op, json!(v + adjust)))
}

fn parse_age(s: &str) -> Option<(Op, Value)> {
    let s = s.trim();
    let (op, rest) = if let Some(r) = s.strip_prefix("<=").or(s.strip_prefix('<')) {
        (Op::LessThan, r)
    } else if let Some(r) = s.strip_prefix(">=").or(s.strip_prefix('>')) {
        (Op::GreaterThan, r)
    } else {
        (Op::LessThan, s)
    };
    let rest = rest.trim();
    let (num, unit) = rest.split_at(rest.find(|c: char| c.is_alphabetic()).unwrap_or(rest.len()));
    let n: f64 = num.trim().parse().ok()?;
    let days = match unit.trim() {
        "" | "d" => n,
        "w" => n * 7.0,
        "m" => n * 30.0,
        "y" => n * 365.0,
        _ => return None,
    };
    Some((op, json!(days)))
}

/// Parses rule text. Returns the error message for the first bad condition.
pub fn parse(text: &str) -> Result<Rule, String> {
    let mut rules = Vec::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (negate, part) = match part.strip_prefix('-') {
            Some(p) => (true, p.trim()),
            None => (false, part),
        };
        let (name, rest) = part.split_once(char::is_whitespace).unwrap_or((part, ""));
        let rest = rest.trim();
        let bad = || format!("Cannot read “{part}”");
        let rule = match name.to_ascii_lowercase().as_str() {
            n @ ("bpm" | "rating" | "year" | "plays" | "duration") => {
                let field = match n {
                    "bpm" => Field::Bpm,
                    "rating" => Field::Rating,
                    "year" => Field::Year,
                    "plays" => Field::PlayCount,
                    _ => Field::Duration,
                };
                let (op, value) = parse_number_expr(rest).ok_or_else(bad)?;
                Rule::Cond { field, op, value }
            }
            "key" => {
                let harmonic = rest.ends_with('~');
                let k = rest.trim_end_matches('~').trim();
                od_core::MusicalKey::parse(k).ok_or_else(bad)?;
                Rule::Cond {
                    field: Field::Key,
                    op: if harmonic { Op::HarmonicWith } else { Op::Is },
                    value: json!(k.to_ascii_uppercase()),
                }
            }
            "added" => {
                let (op, value) = parse_age(rest).ok_or_else(bad)?;
                Rule::Cond {
                    field: Field::AddedDaysAgo,
                    op,
                    value,
                }
            }
            n @ ("title" | "artist" | "album" | "genre" | "label" | "comment" | "format") => {
                if rest.is_empty() {
                    return Err(bad());
                }
                let field = match n {
                    "title" => Field::Title,
                    "artist" => Field::Artist,
                    "album" => Field::Album,
                    "genre" => Field::Genre,
                    "label" => Field::Label,
                    "comment" => Field::Comment,
                    _ => Field::Format,
                };
                Rule::Cond {
                    field,
                    op: if negate {
                        Op::NotContains
                    } else {
                        Op::Contains
                    },
                    value: json!(rest),
                }
            }
            _ => {
                return Err(format!(
                    "Unknown field “{name}”. Use bpm, key, genre, artist, title, album, label, comment, rating, year, plays, added or format."
                ));
            }
        };
        rules.push(rule);
    }
    Ok(Rule::All(rules))
}

fn number(v: &Value) -> String {
    let f = v.as_f64().unwrap_or(0.0);
    let r = (f * 1000.0).round() / 1000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}

/// Formats a rule back into text (inverse of [`parse`] for rules it produced).
pub fn format(rule: &Rule) -> String {
    let Rule::All(rules) = rule else {
        return String::new();
    };
    rules
        .iter()
        .filter_map(|r| {
            let Rule::Cond { field, op, value } = r else {
                return None;
            };
            let name = match field {
                Field::Title => "title",
                Field::Artist => "artist",
                Field::Album => "album",
                Field::Genre => "genre",
                Field::Label => "label",
                Field::Comment => "comment",
                Field::Bpm => "bpm",
                Field::Key => "key",
                Field::Rating => "rating",
                Field::Year => "year",
                Field::AddedDaysAgo => "added",
                Field::PlayCount => "plays",
                Field::Duration => "duration",
                Field::Color => "color",
                Field::Format => "format",
            };
            let text = value.as_str().map(str::to_string);
            Some(match op {
                Op::Contains => format!("{name} {}", text.unwrap_or_default()),
                Op::NotContains => format!("-{name} {}", text.unwrap_or_default()),
                Op::HarmonicWith => format!("{name} {}~", text.unwrap_or_default()),
                Op::Is if *field == Field::Key => format!("{name} {}", text.unwrap_or_default()),
                Op::Between => {
                    let a = value.as_array().cloned().unwrap_or_default();
                    format!(
                        "{name} {}-{}",
                        number(a.first().unwrap_or(&json!(0))),
                        number(a.get(1).unwrap_or(&json!(0)))
                    )
                }
                Op::GreaterThan if *field == Field::AddedDaysAgo => {
                    format!("{name} > {}d", number(value))
                }
                Op::LessThan if *field == Field::AddedDaysAgo => {
                    format!("{name} < {}d", number(value))
                }
                Op::GreaterThan => {
                    let v = value.as_f64().unwrap_or(0.0);
                    if (v - v.round()).abs() < 1e-6 {
                        format!("{name} >= {}", number(value))
                    } else {
                        format!("{name} > {}", number(value))
                    }
                }
                Op::LessThan => {
                    let v = value.as_f64().unwrap_or(0.0);
                    if (v - v.round()).abs() < 1e-6 {
                        format!("{name} <= {}", number(value))
                    } else {
                        format!("{name} < {}", number(value))
                    }
                }
                Op::Is | Op::IsNot => format!("{name} {}", text.unwrap_or_else(|| number(value))),
            })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        let r = parse("bpm 120-126, key 8a~, genre house, -genre techno, rating >= 3, added < 30d")
            .unwrap();
        let Rule::All(v) = &r else { panic!() };
        assert_eq!(v.len(), 6);
        assert_eq!(
            format(&r),
            "bpm 120-126, key 8A~, genre house, -genre techno, rating >= 3, added < 30d"
        );
        assert!(parse("tempo 120").is_err());
        assert!(parse("key zz").is_err());
        assert_eq!(parse("").unwrap(), Rule::All(vec![]));
    }
}
