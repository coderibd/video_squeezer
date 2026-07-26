//! User-selected processing settings.

use std::path::PathBuf;

/// Video codec requested for newly encoded files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
}

/// Determines whether FFmpeg should use Apple hardware encoding or CPU encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderMode {
    Auto,
    VideoToolbox,
    Software,
}

/// A validated snapshot of all settings required by worker threads.
///
/// The GUI itself is not thread-safe, so workers never read controls directly.
/// Instead, the app converts the current controls into this plain Rust value
/// before starting the scheduler.
#[derive(Clone)]
pub struct JobConfig {
    pub input: PathBuf,
    pub output: PathBuf,
    pub include_subdirectories: bool,
    pub target_mib: u64,
    pub max_width: u32,
    pub max_height: u32,
    pub codec: Codec,
    pub encoder_mode: EncoderMode,
    pub software_preset: String,
    pub audio_kbps: u32,
    pub size_margin: f64,
    pub jobs: usize,
    pub overwrite: bool,
    pub make_contact_sheet: bool,
    pub skip_compliant: bool,
    pub use_hardware: bool,
}
