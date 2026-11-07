//! The quick filter bar: BPM range, minimum rating, one My Tag value, and
//! key / tempo relative to the master deck.

use od_core::MusicalKey;
use od_library::{MyTag, TrackRecord};

/// The track the master-relative conditions compare with.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Reference {
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    /// Shown in the bar, e.g. "MASTER 124.0 · 8A".
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Filter {
    pub bpm_min: Option<f64>,
    pub bpm_max: Option<f64>,
    /// Minimum stars, 0 = any.
    pub rating_min: u8,
    pub my_tag: Option<MyTag>,
    /// Key compatible with the reference (Camelot distance ≤ 1).
    pub key_match: bool,
    /// Tempo within ± this percentage of the reference (double / half time count).
    pub bpm_pct: Option<f64>,
}

fn number(v: &str) -> Option<f64> {
    v.trim()
        .trim_start_matches('±')
        .trim_end_matches('%')
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite() && *x >= 0.0)
}

/// "Category: Value" → My Tag ("" or no value = none).
pub(crate) fn parse_my_tag(v: &str) -> Option<MyTag> {
    let (c, v) = v.split_once(':')?;
    let t = MyTag::new(c, v);
    (!t.category.is_empty() && !t.value.is_empty()).then_some(t)
}

pub(crate) fn my_tag_text(t: &MyTag) -> String {
    format!("{}: {}", t.category, t.value)
}

impl Filter {
    /// Any condition set.
    pub fn active(&self) -> bool {
        self.bpm_min.is_some()
            || self.bpm_max.is_some()
            || self.rating_min > 0
            || self.my_tag.is_some()
            || self.key_match
            || self.bpm_pct.is_some()
    }

    /// Conditions that need a reference track.
    pub fn uses_reference(&self) -> bool {
        self.key_match || self.bpm_pct.is_some()
    }

    /// Applies one edit from the filter bar.
    pub fn edit(&mut self, field: &str, value: &str) {
        match field {
            "bpm-min" => self.bpm_min = number(value),
            "bpm-max" => self.bpm_max = number(value),
            "pct" => self.bpm_pct = number(value),
            "key" => self.key_match = !value.is_empty(),
            "rating" => self.rating_min = value.trim().parse::<u8>().unwrap_or(0).min(5),
            "my-tag" => self.my_tag = parse_my_tag(value),
            "clear" => *self = Filter::default(),
            _ => {}
        }
    }

    /// Whether `t` (with its My Tag values) passes. Master-relative
    /// conditions are skipped without a reference (or its BPM / key).
    pub fn matches(&self, t: &TrackRecord, tags: &[MyTag], reference: Option<&Reference>) -> bool {
        if let Some(lo) = self.bpm_min
            && !t.bpm.is_some_and(|b| b >= lo)
        {
            return false;
        }
        if let Some(hi) = self.bpm_max
            && !t.bpm.is_some_and(|b| b <= hi)
        {
            return false;
        }
        if t.tags.rating < self.rating_min {
            return false;
        }
        if let Some(m) = &self.my_tag
            && !tags.contains(m)
        {
            return false;
        }
        if let Some(r) = reference {
            if let (true, Some(rk)) = (self.key_match, r.key)
                && !t.key.is_some_and(|k| rk.camelot_distance(k) <= 1)
            {
                return false;
            }
            if let (Some(p), Some(rb)) = (self.bpm_pct, r.bpm)
                && !t
                    .bpm
                    .is_some_and(|b| od_library::sets::bpm_change_pct(rb, b).abs() <= p + 1e-9)
            {
                return false;
            }
        }
        true
    }

    /// The bar's view model.
    pub fn view(&self, reference: Option<&Reference>) -> crate::FilterView {
        let num = |v: Option<f64>| {
            v.map(|x| {
                if x.fract() == 0.0 {
                    format!("{x:.0}")
                } else {
                    format!("{x}")
                }
            })
            .unwrap_or_default()
        };
        crate::FilterView {
            active: self.active(),
            bpm_min: num(self.bpm_min).into(),
            bpm_max: num(self.bpm_max).into(),
            rating: self.rating_min as i32,
            my_tag: self
                .my_tag
                .as_ref()
                .map(my_tag_text)
                .unwrap_or_default()
                .into(),
            key_match: self.key_match,
            bpm_pct: num(self.bpm_pct).into(),
            reference: reference
                .map(|r| r.label.clone())
                .unwrap_or_default()
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(bpm: Option<f64>, key: Option<&str>, rating: u8) -> TrackRecord {
        let mut t: TrackRecord = serde_json::from_value(serde_json::json!({
            "id": 1, "path": "", "content_hash": "", "file_size": 0, "format": null, "duration": 0.0,
            "sample_rate": 44100, "channels": 2, "bit_rate": null, "bits_per_sample": null,
            "tags": od_core::TrackTags::default(), "date_added": "", "play_count": 0, "last_played": null,
            "missing": false, "bpm": null, "key": null, "analysis_version": 0
        }))
        .unwrap();
        t.bpm = bpm;
        t.key = key.and_then(MusicalKey::parse);
        t.tags.rating = rating;
        t
    }

    #[test]
    fn bpm_range_rating_and_my_tag() {
        let mut f = Filter::default();
        assert!(!f.active());
        f.edit("bpm-min", "120");
        f.edit("bpm-max", "126,5");
        assert_eq!((f.bpm_min, f.bpm_max), (Some(120.0), Some(126.5)));
        assert!(f.matches(&track(Some(124.0), None, 0), &[], None));
        assert!(!f.matches(&track(Some(128.0), None, 0), &[], None));
        assert!(
            !f.matches(&track(None, None, 0), &[], None),
            "unanalysed tracks fail a BPM range"
        );
        f.edit("rating", "3");
        assert!(!f.matches(&track(Some(124.0), None, 2), &[], None));
        assert!(f.matches(&track(Some(124.0), None, 4), &[], None));
        f.edit("my-tag", "Mood: Peak");
        let peak = [MyTag::new("Mood", "Peak")];
        assert!(f.matches(&track(Some(124.0), None, 4), &peak, None));
        assert!(!f.matches(
            &track(Some(124.0), None, 4),
            &[MyTag::new("Mood", "Warm")],
            None
        ));
        assert_eq!(f.view(None).my_tag, "Mood: Peak");
        f.edit("clear", "");
        assert_eq!(f, Filter::default());
        assert_eq!(parse_my_tag("no colon"), None);
    }

    #[test]
    fn key_and_tempo_relative_to_master() {
        let mut f = Filter::default();
        f.edit("key", "1");
        f.edit("pct", "±4%");
        assert!(f.uses_reference() && f.bpm_pct == Some(4.0));
        let master = Reference {
            bpm: Some(125.0),
            key: MusicalKey::parse("8A"),
            label: "MASTER".into(),
        };
        assert!(
            f.matches(&track(Some(124.0), Some("9A"), 0), &[], Some(&master)),
            "adjacent key, 0.8 % slower"
        );
        assert!(
            f.matches(&track(Some(63.0), Some("8B"), 0), &[], Some(&master)),
            "relative key, half time"
        );
        assert!(
            !f.matches(&track(Some(124.0), Some("10A"), 0), &[], Some(&master)),
            "two steps away"
        );
        assert!(
            !f.matches(&track(Some(115.0), Some("8A"), 0), &[], Some(&master)),
            "8 % slower"
        );
        // No master deck: the relative conditions do not hide anything.
        assert!(f.matches(&track(Some(90.0), Some("1B"), 0), &[], None));
    }
}
