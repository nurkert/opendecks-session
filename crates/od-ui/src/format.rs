//! Pure text formatting shared by the view models.

/// "m:ss", or "h:mm:ss" from one hour on (lists, totals).
pub fn time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "m:ss.mmm" (cue points, player position).
pub fn cue_time(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

/// "m:ss.t" (deck clocks).
pub fn deck_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let s = secs as u64;
    format!("{}:{:02}.{}", s / 60, s % 60, (secs.fract() * 10.0) as u64)
}

/// "1.2 GB" / "640 MB".
pub fn bytes(b: u64) -> String {
    let gb = b as f64 / 1e9;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", b as f64 / 1e6)
    }
}

/// Loop length in beats: "1/4", "1", "16".
pub fn beats(b: f64) -> String {
    if b < 1.0 {
        format!("1/{}", (1.0 / b).round())
    } else {
        format!("{b}")
    }
}

/// Remaining time at the deck's tempo: what is left of the track divided by
/// the tempo ratio ("-6:21.3" for 6:49.2 left at +8 %); empty without a track.
/// A stopped or reversed tempo shows the track time.
pub fn remaining_at_tempo(duration: f64, position: f64, tempo: f64) -> String {
    if duration <= 0.0 {
        return String::new();
    }
    let left = (duration - position).max(0.0);
    let t = if tempo.is_finite() && tempo > 0.05 { tempo } else { 1.0 };
    format!("-{}", deck_time(left / t))
}

/// Tempo ratio as pitch offset: 1.015 → "+1.5%", 0.97 → "−3.0%". The minus
/// is U+2212, as wide as the plus, so the digits stay put when the sign flips.
pub fn tempo(ratio: f64) -> String {
    format!("{:+.1}%", (ratio - 1.0) * 100.0).replace('-', "\u{2212}")
}

/// Bar.beat counter: beat 130 in bar position 3 → "33.3"; empty without grid.
pub fn bar_beat(beat: f64, beat_in_bar: u8) -> String {
    if beat_in_bar > 0 {
        format!("{}.{}", (beat.max(0.0) as u64) / 4 + 1, beat_in_bar)
    } else {
        String::new()
    }
}

/// Countdown to a target beat as "-bars.beats", e.g. "-7.4" = 7 bars and
/// 4 beats left; empty when the target is behind the playhead.
pub fn bar_countdown(beat: f64, target_beat: f64) -> String {
    let left = (target_beat - beat.max(0.0).floor()).round() as i64;
    if left <= 0 {
        return String::new();
    }
    let (bars, beats) = ((left - 1) / 4, (left - 1) % 4 + 1);
    format!("-{bars}.{beats}")
}

/// BPM with the given decimals, empty when unknown or zero.
pub fn bpm(b: Option<f64>, decimals: usize) -> String {
    match b {
        Some(b) if b > 0.0 => format!("{b:.decimals$}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times() {
        assert_eq!(time(0.0), "0:00");
        assert_eq!(time(-3.0), "0:00");
        assert_eq!(time(65.4), "1:05");
        assert_eq!(time(3725.0), "1:02:05");
        assert_eq!(cue_time(61.2345), "1:01.235");
        assert_eq!(deck_time(200.65), "3:20.6");
        assert_eq!(deck_time(-1.0), "0:00.0");
    }

    #[test]
    fn sizes_and_beats() {
        assert_eq!(bytes(2_500_000_000), "2.5 GB");
        assert_eq!(bytes(640_000_000), "640 MB");
        assert_eq!(beats(0.25), "1/4");
        assert_eq!(beats(4.0), "4");
        assert_eq!(beats(0.5), "1/2");
    }

    #[test]
    fn deck_fields() {
        assert_eq!(remaining_at_tempo(409.2, 0.0, 1.08), format!("-{}", deck_time(409.2 / 1.08)));
        assert_eq!(remaining_at_tempo(60.0, 30.0, 1.0), "-0:30.0");
        assert_eq!(remaining_at_tempo(60.0, 30.0, 0.0), "-0:30.0");
        assert_eq!(remaining_at_tempo(0.0, 0.0, 1.0), "");
        assert_eq!(tempo(1.0), "+0.0%");
        assert_eq!(tempo(0.97), "\u{2212}3.0%");
        assert_eq!(bar_beat(130.0, 3), "33.3");
        assert_eq!(bar_beat(130.0, 0), "");
        assert_eq!(bpm(Some(128.0), 1), "128.0");
        assert_eq!(bpm(Some(0.0), 2), "");
        assert_eq!(bpm(None, 2), "");
    }

    #[test]
    fn bar_countdown_counts_bars_and_beats_left() {
        // At beat 0 with the target at beat 32: 8 bars left, shown -7.4.
        assert_eq!(super::bar_countdown(0.0, 32.0), "-7.4");
        assert_eq!(super::bar_countdown(31.2, 32.0), "-0.1");
        assert_eq!(super::bar_countdown(32.0, 32.0), "");
    }
}
