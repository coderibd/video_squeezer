//! Converts GUI controls into validated worker settings.

use crate::{
    models::{Codec, EncoderMode, JobConfig, QualityStrategy},
    AppWindow,
};
use anyhow::Result;
use std::path::PathBuf;

/// Reads the current controls and produces an immutable job configuration.
pub fn from_ui(ui: &AppWindow) -> Result<JobConfig> {
    let input = PathBuf::from(ui.get_input_path().to_string());
    let output = PathBuf::from(ui.get_output_path().to_string());

    anyhow::ensure!(input.is_dir(), "Choose a valid input folder.");
    anyhow::ensure!(!output.as_os_str().is_empty(), "Choose an output folder.");

    let (max_width, max_height) = match ui.get_max_resolution().as_str() {
        value if value.starts_with("854x480") => (854, 480),
        value if value.starts_with("1920x1080") => (1920, 1080),
        value if value.starts_with("3840x2160") => (3840, 2160),
        _ => (1280, 720),
    };

    let codec = if ui.get_codec().contains("264") {
        Codec::H264
    } else {
        Codec::H265
    };

    let encoder_mode = match ui.get_encoder().as_str() {
        value if value.starts_with("Apple") => EncoderMode::VideoToolbox,
        value if value.starts_with("Software") => EncoderMode::Software,
        _ => EncoderMode::Auto,
    };

    let software_preset = ui
        .get_preset()
        .to_string()
        .to_ascii_lowercase()
        .replace(' ', "");
    let audio_kbps = ui
        .get_audio_bitrate()
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(128);

    let quality_strategy = match ui.get_quality_strategy().as_str() {
        value if value.starts_with("Best") => QualityStrategy::BestQuality,
        value if value.starts_with("Smallest") => QualityStrategy::SmallestFile,
        value if value.starts_with("Fastest") => QualityStrategy::FastestEncode,
        _ => QualityStrategy::Balanced,
    };

    Ok(JobConfig {
        input,
        output,
        include_subdirectories: ui.get_include_subdirectories(),
        target_mib: ui.get_target_mib().max(50) as u64,
        max_width,
        max_height,
        codec,
        encoder_mode,
        software_preset,
        audio_kbps,
        size_margin: ui.get_size_margin().clamp(0.0, 0.25) as f64,
        jobs: resolve_concurrency(ui),
        overwrite: ui.get_overwrite_existing(),
        make_contact_sheet: ui.get_generate_contact_sheet(),
        skip_compliant: ui.get_skip_compliant(),
        use_hardware: ui.get_use_hardware(),
        quality_strategy,
        retry_missed_target: ui.get_retry_missed_target(),
        max_encode_attempts: ui.get_max_encode_attempts().clamp(1, 3) as usize,
    })
}

/// Chooses a conservative automatic worker count or returns the manual value.
fn resolve_concurrency(ui: &AppWindow) -> usize {
    if !ui.get_auto_jobs() {
        return ui.get_jobs().clamp(1, 16) as usize;
    }

    let logical_cpus = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(4);
    let software_forced = ui.get_encoder().starts_with("Software") || !ui.get_use_hardware();

    if software_forced {
        (logical_cpus / 4).clamp(1, 2)
    } else {
        (logical_cpus / 2).clamp(2, 4)
    }
}
