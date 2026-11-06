//! Sampler: slots, banks and sequencer (SPEC §10.3a).

use crate::app::{App, handler};
use std::rc::Rc;

/// Opens or closes the sampler panel and remembers it.
pub(crate) fn toggle_panel(app: &App) {
    let on = !app.state.borrow().settings.perform.sampler_panel;
    app.state.borrow_mut().change_settings(|s| s.perform.sampler_panel = on);
    app.ui.set_sampler_panel(on);
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.set_sampler_panel(app.state.borrow().settings.perform.sampler_panel);
    ui.on_toggle_sampler_panel(handler!(app, |a| toggle_panel(&a)));
}
