//! FX: Beat FX, Color FX, Pad FX and Release FX panels (SPEC §10.2).

use crate::app::{App, handler};
use std::rc::Rc;

/// Opens or closes the Beat FX panel and remembers it.
pub(crate) fn toggle_panel(app: &App) {
    let on = !app.state.borrow().settings.perform.fx_panel;
    app.state.borrow_mut().change_settings(|s| s.perform.fx_panel = on);
    app.ui.set_fx_panel(on);
}

pub(crate) fn wire(app: &Rc<App>) {
    let ui = &app.ui;
    ui.set_fx_panel(app.state.borrow().settings.perform.fx_panel);
    ui.on_toggle_fx_panel(handler!(app, |a| toggle_panel(&a)));
}
