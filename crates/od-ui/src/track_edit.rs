//! Edit mode in the Prepare player (EDIT tab): a region of the track (IN /
//! OUT at the cursor, or shift-drag on the overview) is cut out, trimmed to
//! or repeated, and the result is rendered into a NEW WAV file next to the
//! original, added to the collection as "<title> (Edit)" and analysed.
//! The original file and its library entry stay untouched.

use crate::app::{App, handler};
use od_core::TrackId;
use slint::{ComponentHandle, SharedString};
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The region of the current track (seconds); either end may be unset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Region {
    pub track: Option<TrackId>,
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl Region {
    /// The region as (start, end) seconds for `track` when both ends are set
    /// and apart.
    pub(crate) fn span(&self, track: Option<TrackId>) -> Option<(f64, f64)> {
        if self.track != track || track.is_none() {
            return None;
        }
        let (a, b) = (self.start?, self.end?);
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        (e - s > 0.05).then_some((s, e))
    }
}

/// What happens to the region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditOp {
    /// Everything but the region.
    Cut,
    /// Only the region.
    Trim,
    /// The region twice in a row.
    Repeat,
}

impl EditOp {
    /// The status while the edit renders.
    fn rendering(self) -> &'static str {
        match self {
            EditOp::Cut => "Rendering the edit without the region…",
            EditOp::Trim => "Rendering the edit with only the region…",
            EditOp::Repeat => "Rendering the edit with the region repeated…",
        }
    }
}

/// Crossfade at every join and fade at a trimmed edge (seconds).
const FADE: f64 = 0.005;

/// The source frame ranges that make up the result of `op` on a region of
/// frames `start..end` in a file of `frames` frames (pure).
pub(crate) fn segments(op: EditOp, start: usize, end: usize, frames: usize) -> Vec<(usize, usize)> {
    let (s, e) = (start.min(frames), end.min(frames).max(start.min(frames)));
    let v = match op {
        EditOp::Cut => vec![(0, s), (e, frames)],
        EditOp::Trim => vec![(s, e)],
        EditOp::Repeat => vec![(0, e), (s, e), (e, frames)],
    };
    v.into_iter().filter(|(a, b)| b > a).collect()
}

/// Joins the source `segments` of interleaved `samples` (`channels`
/// channels). Every join crossfades over `fade` frames from what would have
/// followed the previous segment into the next one, so the next segment
/// starts exactly at the join (timing and grid stay intact); the result
/// fades in / out over `fade` frames where it starts or ends inside the
/// source (pure).
pub(crate) fn render(samples: &[f32], channels: usize, segments: &[(usize, usize)], fade: usize) -> Vec<f32> {
    let c = channels.max(1);
    let frames = samples.len() / c;
    let at = |f: usize, ch: usize| if f < frames { samples[f * c + ch] } else { 0.0 };
    let mut out: Vec<f32> = Vec::with_capacity(segments.iter().map(|(a, b)| (b - a) * c).sum());
    let mut prev_end: Option<usize> = None;
    for &(s, e) in segments {
        let n = match prev_end {
            Some(_) => fade.min(e - s),
            None => 0,
        };
        for k in 0..(e - s) {
            for ch in 0..c {
                let x = at(s + k, ch);
                let v = match prev_end {
                    Some(pe) if k < n => {
                        let g = (k as f32 + 0.5) / n as f32;
                        x * g + at(pe + k, ch) * (1.0 - g)
                    }
                    _ => x,
                };
                out.push(v);
            }
        }
        prev_end = Some(e);
    }
    let len = out.len() / c;
    let ramp = |out: &mut Vec<f32>, k: usize, g: f32| (0..c).for_each(|ch| out[k * c + ch] *= g);
    if segments.first().is_some_and(|(s, _)| *s > 0) {
        let n = fade.min(len);
        (0..n).for_each(|k| ramp(&mut out, k, (k as f32 + 0.5) / n as f32));
    }
    if segments.last().is_some_and(|(_, e)| *e < frames) {
        let n = fade.min(len);
        (0..n).for_each(|k| ramp(&mut out, len - 1 - k, (k as f32 + 0.5) / n as f32));
    }
    out
}

/// "<stem> (Edit).wav" next to `original`, or "(Edit 2)", "(Edit 3)" … when
/// taken.
pub(crate) fn edit_path(original: &Path) -> PathBuf {
    let dir = original.parent().unwrap_or(Path::new("."));
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Track".into());
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{stem} (Edit).wav") } else { format!("{stem} (Edit {n}).wav") }))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} (Edit).wav")))
}

/// Writes interleaved float samples as 24-bit PCM WAV (the format players
/// and every DJ application read).
pub(crate) fn write_wav(path: &Path, samples: &[f32], channels: u16, rate: u32) -> anyhow::Result<()> {
    let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: 24, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec)?;
    const MAX: f32 = 8_388_607.0;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * MAX).round() as i32)?;
    }
    w.finalize()?;
    Ok(())
}

/// The title of an edit of a track called `title` (the file name when the
/// track has no title).
pub(crate) fn edit_title(title: &str, path: &Path) -> String {
    let base = if title.trim().is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { title.trim().to_string() };
    format!("{base} (Edit)")
}

/// Decodes `source`, renders `op` on `start..end` seconds into a new WAV
/// file and adds it to the library at `library` with the tags of `track`.
/// Runs on a worker thread; returns the new track.
pub(crate) fn run_edit(library: &Path, track: TrackId, op: EditOp, start: f64, end: f64) -> anyhow::Result<TrackId> {
    let mut lib = od_library::Library::open(library)?;
    let t = lib.track(track)?;
    let audio = od_decode::decode(&t.path, od_decode::Layout::Stereo)?;
    let rate = audio.sample_rate.max(1);
    let frame = |s: f64| (s.max(0.0) * rate as f64).round() as usize;
    let segs = segments(op, frame(start), frame(end), audio.frames());
    let fade = (FADE * rate as f64).round() as usize;
    let out = render(&audio.samples, audio.channels as usize, &segs, fade);
    if out.is_empty() {
        anyhow::bail!("the edit would be empty");
    }
    let path = edit_path(&t.path);
    write_wav(&path, &out, audio.channels, rate)?;
    let info = od_decode::probe(&path)?;
    let mut tags = t.tags.clone();
    tags.title = edit_title(&t.tags.title, &t.path);
    let artwork = lib.artwork(track).ok().flatten();
    let id = lib.add_track(&od_library::NewTrack {
        path: path.clone(),
        content_hash: od_decode::content_hash(&path)?,
        file_size: std::fs::metadata(&path)?.len(),
        format: info.format,
        duration: info.duration.unwrap_or(0.0),
        sample_rate: info.sample_rate,
        channels: info.channels,
        bit_rate: info.bit_rate_kbps,
        bits_per_sample: info.bits_per_sample,
        tags,
        artwork,
    })?;
    Ok(id)
}

type Done = crossbeam_channel::Receiver<Result<TrackId, String>>;

thread_local! {
    /// The edit being rendered and the timer that waits for it.
    static JOB: std::cell::RefCell<Option<(Done, slint::Timer)>> = const { std::cell::RefCell::new(None) };
    /// The app, for the timer that collects a rendered edit.
    static APP: std::cell::RefCell<std::rc::Weak<App>> = const { std::cell::RefCell::new(std::rc::Weak::new()) };
}

/// Whether an edit is being rendered.
pub(crate) fn busy() -> bool {
    JOB.with(|j| j.borrow().is_some())
}

/// Sets the region start (`end == false`) or end at the cursor.
pub(crate) fn mark(app: &App, end: bool) {
    {
        let mut st = app.state.borrow_mut();
        let (cur, cursor) = (st.player.current, st.player.cursor);
        let Some(_) = cur else { return };
        let r = &mut st.player.edit;
        if r.track != cur {
            *r = Region { track: cur, ..Default::default() };
        }
        if end { r.end = Some(cursor) } else { r.start = Some(cursor) }
    }
    refresh(app);
}

/// The region dragged on the overview (fractions of the track), snapped to
/// beats.
pub(crate) fn select(app: &App, a: f32, b: f32) {
    {
        let mut st = app.state.borrow_mut();
        let Some(id) = st.player.current else { return };
        let duration = st.src().track(id).map(|t| t.duration).unwrap_or(0.0);
        let grid = st.src().beatgrid(id).ok().flatten();
        let snap = |f: f32| {
            let t = (f.clamp(0.0, 1.0) as f64) * duration;
            grid.as_ref().map(|g| crate::player::snap_to_beat(g, t)).unwrap_or(t)
        };
        st.player.edit = Region { track: Some(id), start: Some(snap(a.min(b))), end: Some(snap(a.max(b))) };
    }
    refresh(app);
}

pub(crate) fn clear(app: &App) {
    app.state.borrow_mut().player.edit = Region::default();
    refresh(app);
}

/// Renders `op` on the region into a new file (in the background).
pub(crate) fn apply(app: &App, op: EditOp) {
    if app.guard_read_only() {
        return;
    }
    if busy() {
        app.status("An edit is being rendered — one moment");
        return;
    }
    let (span, track, library) = {
        let st = app.state.borrow();
        (st.player.edit.span(st.player.current), st.player.current, st.config.library_path.clone())
    };
    let (Some((start, end)), Some(track)) = (span, track) else {
        app.status("Set IN and OUT on the waveform first");
        return;
    };
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        let r = run_edit(&library, track, op, start, end).map_err(|e| e.to_string());
        let _ = tx.send(r);
    });
    app.status(op.rendering());
    let timer = slint::Timer::default();
    let weak = APP.with(|a| a.borrow().clone());
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(50), move || {
        let Some(app) = weak.upgrade() else { return };
        let done = JOB.with(|j| j.borrow().as_ref().and_then(|(rx, _)| rx.try_recv().ok()));
        if let Some(result) = done {
            JOB.with(|j| j.borrow_mut().take());
            finished(&app, result);
        }
    });
    JOB.with(|j| *j.borrow_mut() = Some((rx, timer)));
    refresh(app);
}

/// The rendered edit is in the library: show it in the player, analyse it.
fn finished(app: &App, result: Result<TrackId, String>) {
    match result {
        Ok(id) => {
            {
                let mut st = app.state.borrow_mut();
                st.player.current = Some(id);
                st.player.cursor = 0.0;
                st.player.edit = Region::default();
            }
            let _ = app.analysis.wake.send(());
            app.refresh_library();
            let title = app.state.borrow().lib.track(id).map(|t| t.tags.title).unwrap_or_default();
            app.status(od_i18n::trf("Edit saved as a new track: {}", &[&title]));
        }
        Err(e) => app.status(od_i18n::trf("The edit failed: {}", &[&e])),
    }
    refresh(app);
}

/// The EDIT tab: region readout and its place on the overview.
pub(crate) fn refresh(app: &App) {
    let st = app.state.borrow();
    let te = app.ui.global::<crate::TrackEdit>();
    let r = st.player.edit;
    let cur = st.player.current;
    let duration = cur.and_then(|id| st.src().track(id).ok()).map(|t| t.duration).unwrap_or(0.0).max(0.001);
    let mine = r.track.is_some() && r.track == cur;
    let frac = |t: Option<f64>| t.filter(|_| mine).map(|t| (t / duration) as f32).unwrap_or(-1.0);
    te.set_start(frac(r.start));
    te.set_end(frac(r.end));
    te.set_ready(r.span(cur).is_some());
    te.set_busy(busy());
    let text = match (mine.then_some(r.start).flatten(), mine.then_some(r.end).flatten()) {
        (Some(a), Some(b)) => {
            let (s, e) = (a.min(b), a.max(b));
            format!("{} – {}  ({})", crate::format::cue_time(s), crate::format::cue_time(e), crate::format::time(e - s))
        }
        (Some(a), None) => format!("{} –", crate::format::cue_time(a)),
        (None, Some(b)) => format!("– {}", crate::format::cue_time(b)),
        (None, None) => String::new(),
    };
    te.set_region(SharedString::from(text));
}

pub(crate) fn wire(app: &Rc<App>) {
    APP.with(|a| *a.borrow_mut() = Rc::downgrade(app));
    let te = app.ui.global::<crate::TrackEdit>();
    te.on_mark_in(handler!(app, |a| mark(&a, false)));
    te.on_mark_out(handler!(app, |a| mark(&a, true)));
    te.on_select(handler!(app, |a, x: f32, y: f32| select(&a, x, y)));
    te.on_clear(handler!(app, |a| clear(&a)));
    te.on_apply(handler!(app, |a, op: SharedString| match op.as_str() {
        "cut" => apply(&a, EditOp::Cut),
        "trim" => apply(&a, EditOp::Trim),
        "repeat" => apply(&a, EditOp::Repeat),
        _ => {}
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_per_operation() {
        assert_eq!(segments(EditOp::Cut, 10, 20, 50), vec![(0, 10), (20, 50)]);
        assert_eq!(segments(EditOp::Trim, 10, 20, 50), vec![(10, 20)]);
        assert_eq!(segments(EditOp::Repeat, 10, 20, 50), vec![(0, 20), (10, 20), (20, 50)]);
        // A region at the very start or end leaves no empty segment.
        assert_eq!(segments(EditOp::Cut, 0, 20, 50), vec![(20, 50)]);
        assert_eq!(segments(EditOp::Cut, 30, 80, 50), vec![(0, 30)]);
    }

    #[test]
    fn render_joins_with_crossfades_and_keeps_timing() {
        // Mono ramp 0..100: sample i has value i.
        let src: Vec<f32> = (0..100).map(|i| i as f32).collect();
        // Cut 20..40: 80 frames; the join starts exactly at frame 20 with
        // source frame 40 (faded in over 4 frames from 20.. continuing).
        let out = render(&src, 1, &segments(EditOp::Cut, 20, 40, 100), 4);
        assert_eq!(out.len(), 80);
        assert_eq!(&out[..20], &src[..20], "untouched before the cut");
        assert_eq!(&out[24..], &src[44..], "untouched after the crossfade");
        let g = 0.5 / 4.0;
        assert!((out[20] - (40.0 * g + 20.0 * (1.0 - g))).abs() < 1e-4, "crossfade from the continuation (20) into 40");
        // Repeat 10..30: 120 frames; the second copy starts at frame 30.
        let rep = render(&src, 1, &segments(EditOp::Repeat, 10, 30, 100), 4);
        assert_eq!(rep.len(), 120);
        assert_eq!(&rep[34..50], &src[14..30], "the copy of the region");
        assert_eq!(&rep[54..], &src[34..], "and the rest, shifted by the region length");
        // The join of identical material (second copy → rest) is seamless.
        assert!((rep[50] - src[30]).abs() < 1e-4);
        // Trim 10..30: 20 frames, faded at both edges.
        let trim = render(&src, 1, &segments(EditOp::Trim, 10, 30, 100), 4);
        assert_eq!(trim.len(), 20);
        assert!(trim[0] < src[10] && trim[19] < src[29], "fades at both edges");
        assert_eq!(&trim[4..16], &src[14..26]);
        // Stereo stays interleaved.
        let st: Vec<f32> = (0..20).flat_map(|i| [i as f32, -(i as f32)]).collect();
        let o = render(&st, 2, &[(0, 5), (10, 20)], 0);
        assert_eq!(o.len(), 30);
        assert_eq!((o[10], o[11]), (10.0, -10.0));
    }

    #[test]
    fn region_and_names() {
        let id = Some(TrackId(3));
        let r = Region { track: id, start: Some(20.0), end: Some(10.0) };
        assert_eq!(r.span(id), Some((10.0, 20.0)), "ends in either order");
        assert_eq!(r.span(Some(TrackId(4))), None, "another track");
        assert_eq!(Region { end: None, ..r }.span(id), None);
        assert_eq!(edit_title("Night Drive", Path::new("/m/x.mp3")), "Night Drive (Edit)");
        assert_eq!(edit_title(" ", Path::new("/m/Loop 1.wav")), "Loop 1 (Edit)");
        let dir = tempfile::tempdir().unwrap();
        let orig = dir.path().join("Song.mp3");
        assert_eq!(edit_path(&orig), dir.path().join("Song (Edit).wav"));
        std::fs::write(dir.path().join("Song (Edit).wav"), b"x").unwrap();
        assert_eq!(edit_path(&orig), dir.path().join("Song (Edit 2).wav"));
    }
}
