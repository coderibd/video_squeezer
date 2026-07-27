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

/// Describes how strongly the encoder should favor size, quality, or speed.
///
/// The target size and maximum resolution remain the primary constraints. This
/// strategy changes how aggressively the application responds when those goals
/// conflict with one another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityStrategy {
    /// Try to meet the requested size while retaining reasonable quality.
    Balanced,
    /// Protect a calculated quality floor, even when that may exceed the target.
    BestQuality,
    /// Favor staying below the target and allow a more aggressive retry.
    SmallestFile,
    /// Prefer hardware encoding and faster presets over compression efficiency.
    FastestEncode,
}

impl QualityStrategy {
    /// Human-readable name used in status messages and documentation.
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::BestQuality => "Best Quality",
            Self::SmallestFile => "Smallest File",
            Self::FastestEncode => "Fastest Encode",
        }
    }
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
    pub quality_strategy: QualityStrategy,
    pub retry_missed_target: bool,
    pub max_encode_attempts: usize,
}
