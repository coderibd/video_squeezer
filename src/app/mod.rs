//! Application layer: window lifecycle, callbacks, settings, and UI projection.

mod callbacks;
mod settings;
pub mod view;

use crate::{models::SharedState, AppWindow};
use anyhow::Result;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{cell::Cell, rc::Rc, sync::Arc, time::Duration};

/// Creates the window, connects callbacks, and enters the Slint event loop.
pub fn run() -> Result<()> {
    let ui = AppWindow::new()?;
    let state = Arc::new(SharedState::default());

    callbacks::wire(&ui, state.clone());
    view::refresh_ui(&ui.as_weak(), &state);

    // Slint's standard widget set differs across versions. A Unicode spinner
    // driven by a timer gives reliable loading feedback without depending on a
    // version-specific BusyIndicator component.
    let spinner_timer = Timer::default();
    let spinner_index = Rc::new(Cell::new(0usize));
    let spinner_index_for_timer = spinner_index.clone();
    let spinner_ui = ui.as_weak();
    const SPINNER_FRAMES: [&str; 4] = ["◐", "◓", "◑", "◒"];

    spinner_timer.start(TimerMode::Repeated, Duration::from_millis(120), move || {
        let next = (spinner_index_for_timer.get() + 1) % SPINNER_FRAMES.len();
        spinner_index_for_timer.set(next);
        if let Some(ui) = spinner_ui.upgrade() {
            ui.set_loading_glyph(SPINNER_FRAMES[next].into());
        }
    });

    // Opening maximized ensures the complete settings panel and Start button
    // are visible. The Slint file still permits normal resizing afterward.
    ui.window().set_maximized(true);
    ui.run()?;
    Ok(())
}
