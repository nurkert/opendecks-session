//! Set preparation helpers: flow curves and transition hints.

use crate::nodes::NodeId;
use crate::{Library, Result, TrackRecord};
use od_core::MusicalKey;
use serde::Serialize;

/// How well two neighbouring tracks fit together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    Good,
    Ok,
    Clash,
    Unknown,
}

/// The transition from position `i` to `i + 1`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Transition {
    /// Tempo change in percent needed to match the next track (positive =
    /// faster). Half/double time is taken into account.
    pub bpm_change_pct: Option<f64>,
    /// Camelot distance (0 = same key, 1 = adjacent/relative).
    pub key_distance: Option<u8>,
    pub tempo_fit: Fit,
    pub key_fit: Fit,
}

/// One position of a set with its flow values.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FlowPoint {
    pub track: TrackRecord,
    /// Seconds from the start of the set.
    pub start_offset: f64,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    /// Mean energy 0..=1 from analysis.
    pub energy: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SetFlow {
    pub points: Vec<FlowPoint>,
    /// `transitions[i]` describes points[i] -> points[i + 1].
    pub transitions: Vec<Transition>,
    pub total_duration: f64,
}

/// Tempo change in percent from `a` to `b`, choosing the closest of
/// b, b*2 and b/2.
pub fn bpm_change_pct(a: f64, b: f64) -> f64 {
    let candidates = [b, b * 2.0, b / 2.0];
    candidates
        .iter()
        .map(|c| (c - a) / a * 100.0)
        .min_by(|x, y| x.abs().total_cmp(&y.abs()))
        .unwrap_or(0.0)
}

pub fn transition(a: &FlowPoint, b: &FlowPoint) -> Transition {
    let bpm_change_pct = match (a.bpm, b.bpm) {
        (Some(x), Some(y)) if x > 0.0 && y > 0.0 => Some(bpm_change_pct(x, y)),
        _ => None,
    };
    let key_distance = match (a.key, b.key) {
        (Some(x), Some(y)) => Some(x.camelot_distance(y)),
        _ => None,
    };
    Transition {
        bpm_change_pct,
        key_distance,
        tempo_fit: match bpm_change_pct.map(f64::abs) {
            None => Fit::Unknown,
            Some(p) if p <= 3.0 => Fit::Good,
            Some(p) if p <= 6.0 => Fit::Ok,
            Some(_) => Fit::Clash,
        },
        key_fit: match key_distance {
            None => Fit::Unknown,
            Some(0 | 1) => Fit::Good,
            Some(2) => Fit::Ok,
            Some(_) => Fit::Clash,
        },
    }
}

impl Library {
    /// Flow of a set or playlist: start offsets, BPM, key and energy per
    /// position, plus a fit rating for each transition.
    pub fn set_flow(&self, node: NodeId) -> Result<SetFlow> {
        let tracks = self.node_tracks(node)?;
        let mut points = Vec::with_capacity(tracks.len());
        let mut offset = 0.0;
        for t in tracks {
            let energy = {
                let e = self.analysis(t.id)?.energy;
                (!e.is_empty()).then(|| e.iter().sum::<f32>() / e.len() as f32)
            };
            let duration = t.duration;
            points.push(FlowPoint {
                bpm: t.bpm,
                key: t.key,
                energy,
                start_offset: offset,
                track: t,
            });
            offset += duration;
        }
        let transitions = points
            .windows(2)
            .map(|w| transition(&w[0], &w[1]))
            .collect();
        Ok(SetFlow {
            points,
            transitions,
            total_duration: offset,
        })
    }

    /// Library tracks that fit after `track` (tempo within `max_pct` and
    /// harmonically compatible), best matches first.
    pub fn suggest_next(
        &self,
        track: od_core::TrackId,
        max_pct: f64,
        limit: usize,
    ) -> Result<Vec<TrackRecord>> {
        let current = self.track(track)?;
        let (Some(bpm), Some(key)) = (current.bpm, current.key) else {
            return Ok(vec![]);
        };
        let mut scored: Vec<(f64, TrackRecord)> = self
            .tracks(crate::SortKey::DateAdded, true)?
            .into_iter()
            .filter(|t| t.id != track)
            .filter_map(|t| {
                let pct = bpm_change_pct(bpm, t.bpm?).abs();
                let dist = key.camelot_distance(t.key?);
                (pct <= max_pct && dist <= 1).then_some((pct + dist as f64 * 2.0, t))
            })
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(scored.into_iter().take(limit).map(|(_, t)| t).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;
    use crate::test_util::track;

    #[test]
    fn bpm_change_handles_double_time() {
        assert!((bpm_change_pct(128.0, 64.0)).abs() < 1e-9);
        assert!((bpm_change_pct(120.0, 126.0) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn flow_and_hints() {
        let mut lib = Library::open_in_memory().unwrap();
        let a = track(&mut lib, "A", Some(122.0), Some("8A"));
        let b = track(&mut lib, "B", Some(124.0), Some("9A"));
        let c = track(&mut lib, "C", Some(140.0), Some("3B"));
        let set = lib.create_node(None, NodeKind::Set, "Friday").unwrap();
        lib.add_to_node(set, &[a, b, c], None).unwrap();
        let flow = lib.set_flow(set).unwrap();
        assert_eq!(flow.points.len(), 3);
        assert_eq!(flow.points[2].start_offset, 600.0);
        assert_eq!(flow.total_duration, 900.0);
        assert_eq!(flow.transitions[0].tempo_fit, Fit::Good);
        assert_eq!(flow.transitions[0].key_fit, Fit::Good);
        assert_eq!(flow.transitions[1].tempo_fit, Fit::Clash);
        assert_eq!(flow.transitions[1].key_fit, Fit::Clash);
        let next = lib.suggest_next(a, 3.0, 5).unwrap();
        assert_eq!(next.iter().map(|t| t.id).collect::<Vec<_>>(), vec![b]);
    }
}
