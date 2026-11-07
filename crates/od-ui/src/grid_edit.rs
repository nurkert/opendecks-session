//! Beatgrid editing mode of the Prepare player (EDIT GRID): a guided
//! session on the enlarged waveform. Shift-drag moves the grid (live
//! preview, stored on release), a click on a beat makes it the downbeat,
//! BPM / TAP / ½ / ×2 / ±1 ms / ±10 ms and LOCK sit in the GRID tab. Every
//! change is one undo step; DONE keeps the result, CANCEL restores the grid
//! the session started with. Perform reaches it from each lane's GRID key.

use crate::app::{App, handler};
use crate::player;
use od_core::{Beatgrid, TrackId};
use slint::ComponentHandle;
use std::rc::Rc;

thread_local! {
    /// The grid as it was when the session started (CANCEL restores it).
    static BEFORE: std::cell::RefCell<Option<(TrackId, Beatgrid)>> = const { std::cell::RefCell::new(None) };
}

/// Starts the session on the player's track. False (with a status) when
/// there is nothing to edit.
pub(crate) fn start(app: &App) -> bool {
    if app.guard_read_only() {
        return false;
    }
    let Some(id) = app.state.borrow().player.current else {
        app.status("Select a track to edit its beatgrid");
        return false;
    };
    let grid = app.state.borrow().lib.beatgrid(id).ok().flatten();
    let Some(grid) = grid.filter(|g| !g.beats.is_empty()) else {
        app.status("This track has no beatgrid yet — analyse it first");
        return false;
    };
    if grid.locked {
        app.status("The beatgrid is locked — unlock it (LOCK GRID) to edit");
        return false;
    }
    BEFORE.with(|b| *b.borrow_mut() = Some((id, grid)));
    app.ui.global::<crate::GridEdit>().set_active(true);
    app.status("Edit grid: Shift-drag the waveform until the lines sit on the kicks, click the 1 of a bar");
    true
}

/// DONE: keeps the edits.
pub(crate) fn done(app: &App) {
    end(app);
    app.status("Beatgrid saved");
}

/// CANCEL: back to the grid of the session start (one more undo step).
pub(crate) fn cancel(app: &App) {
    let before = BEFORE.with(|b| b.borrow_mut().take());
    end(app);
    let Some((id, grid)) = before else { return };
    let now = app.state.borrow().lib.beatgrid(id).ok().flatten();
    if now.as_ref() != Some(&grid) {
        let r = app.state.borrow_mut().lib.set_beatgrid(id, grid);
        if app.report(r).is_some() {
            app.after_edit("Beatgrid edit cancelled");
        }
    } else {
        app.status("Beatgrid edit cancelled");
    }
}

fn end(app: &App) {
    BEFORE.with(|b| *b.borrow_mut() = None);
    player::preview_grid_shift(app, 0.0);
    app.ui.global::<crate::GridEdit>().set_active(false);
}

/// Perform: deck `deck`'s track opens in the Prepare player in EDIT GRID.
pub(crate) fn open_deck(app: &App, deck: usize) {
    let track = app.decks.borrow().loaded_id(deck);
    let Some(track) = track else {
        app.status("Load a track on the deck first");
        return;
    };
    {
        let mut st = app.state.borrow_mut();
        st.player.current = Some(track);
    }
    app.ui.invoke_set_mode("prepare".into());
    player::refresh_detail(app);
    start(app);
}

/// The beat nearest to `t` seconds (pure).
pub(crate) fn nearest_beat(grid: &Beatgrid, t: f64) -> Option<usize> {
    let n = grid.beats.len();
    grid.beat_position(t).map(|b| (b.round().max(0.0) as usize).min(n.saturating_sub(1)))
}

pub(crate) fn wire(app: &Rc<App>) {
    let g = app.ui.global::<crate::GridEdit>();
    g.on_start(handler!(app, |a| {
        start(&a);
    }));
    g.on_done(handler!(app, |a| done(&a)));
    g.on_cancel(handler!(app, |a| cancel(&a)));
    g.on_open_deck(handler!(app, |a, d: i32| open_deck(&a, (d.max(1) - 1) as usize)));
    // Drag: `frac` of the enlarged waveform's width (its span).
    g.on_preview(handler!(app, |a, frac: f32| {
        let secs = frac as f64 * player::detail_span(&a);
        player::preview_grid_shift(&a, secs);
    }));
    g.on_commit(handler!(app, |a, frac: f32| {
        let secs = frac as f64 * player::detail_span(&a);
        player::preview_grid_shift(&a, 0.0);
        if secs.abs() >= 0.0005 {
            player::edit_grid(&a, |g, _| g.shift(secs));
        }
    }));
    // A click on the enlarged waveform: `frac` from its centre (the cursor).
    g.on_downbeat_at(handler!(app, |a, frac: f32| {
        let secs = frac as f64 * player::detail_span(&a);
        player::edit_grid(&a, |g, cursor| {
            if let Some(i) = nearest_beat(g, cursor + secs) {
                g.set_downbeat(i);
            }
        });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_beat_takes_the_click() {
        let g = Beatgrid::constant(120.0, 0.0, 1, 20.0);
        assert_eq!(nearest_beat(&g, 2.1), Some(4));
        assert_eq!(nearest_beat(&g, 2.3), Some(5));
        assert_eq!(nearest_beat(&g, -1.0), Some(0));
    }
}
