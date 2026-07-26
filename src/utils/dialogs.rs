//! Native information dialogs.

/// Displays a native macOS information dialog.
pub fn show_message(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("Video Squeezer")
        .set_description(message)
        .set_level(rfd::MessageLevel::Info)
        .show();
}
