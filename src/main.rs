//! Video Squeezer application entry point.
//!
//! This file intentionally contains almost no application logic. Its job is
//! to declare the modules, include the Rust bindings generated from Slint,
//! and hand control to the application layer.

mod app;
mod models;
mod scheduler;
mod services;
mod utils;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    app::run()
}
