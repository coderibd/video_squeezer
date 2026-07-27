//! Compression planning and target-size advice.
//!
//! This module contains no GUI or FFmpeg process code. It converts the user's
//! goals and the video's metadata into a small, testable encoding plan.

use crate::models::{Codec, JobConfig, QualityStrategy};

const MIB: u64 = 1024 * 1024;
const MIN_VIDEO_BPS: u64 = 250_000;

/// The result produced by the Compression Advisor for one video.
#[derive(Debug, Clone)]
pub struct CompressionPlan {
    /// Output dimensions after applying the maximum resolution without upscaling.
    pub output_width: u32,
    pub output_height: u32,
    /// Video bitrate used for the first encoding attempt.
    pub video_bps: u64,
    /// Estimated file size produced by the selected bitrate.
    pub predicted_output_bytes: u64,
    /// Heuristic bitrate below which visible quality loss becomes more likely.
    pub quality_floor_bps: u64,
    /// Friendly quality description displayed by the GUI.
    pub quality_label: &'static str,
    /// Detailed explanation of the trade-off for this video.
    pub message: String,
    /// Whether the source exceeds either the size or resolution constraint.
    pub compression_required: bool,
}

/// Builds a compression plan from metadata and user settings.
///
/// Order matters:
/// 1. Decide the largest permitted output dimensions, never upscaling.
/// 2. Convert the target file size into an available video bitrate.
/// 3. Compare that bitrate with a codec-aware quality floor.
/// 4. Apply the selected strategy.
pub fn build_plan(
    source_width: u32,
    source_height: u32,
    duration_secs: f64,
    fps: f64,
    source_bytes: u64,
    config: &JobConfig,
) -> CompressionPlan {
    let (output_width, output_height) = fit_without_upscaling(
        source_width,
        source_height,
        config.max_width,
        config.max_height,
    );

    let target_bytes = config.target_mib.saturating_mul(MIB);
    let compression_required =
        source_bytes > target_bytes || source_width > output_width || source_height > output_height;

    let target_video_bps = target_video_bitrate(duration_secs, config);
    let quality_floor_bps = quality_floor(
        output_width,
        output_height,
        fps,
        config.codec,
        config.quality_strategy,
    );

    let video_bps = match config.quality_strategy {
        QualityStrategy::BestQuality => target_video_bps.max(quality_floor_bps),
        QualityStrategy::SmallestFile => {
            // Leave an additional buffer so container overhead and bitrate
            // variation are less likely to push the result over the target.
            ((target_video_bps as f64) * 0.95) as u64
        }
        QualityStrategy::Balanced | QualityStrategy::FastestEncode => target_video_bps,
    }
    .max(MIN_VIDEO_BPS);

    let predicted_output_bytes = predicted_size(video_bps, duration_secs, config.audio_kbps);
    let suggested_minimum_bytes =
        predicted_size(quality_floor_bps, duration_secs, config.audio_kbps);
    let ratio = video_bps as f64 / quality_floor_bps.max(1) as f64;
    let quality_label = quality_label(ratio);

    let message = if !compression_required {
        "The source already fits both limits. It can be skipped without re-encoding.".to_owned()
    } else if config.quality_strategy == QualityStrategy::BestQuality
        && predicted_output_bytes > target_bytes
    {
        format!(
            "Best Quality protects the estimated quality floor. The result may be about {} MiB instead of {} MiB.",
            bytes_to_mib(predicted_output_bytes),
            config.target_mib
        )
    } else if ratio < 0.65 {
        format!(
            "The requested size is aggressive for this duration and resolution. About {} MiB is suggested for better quality.",
            bytes_to_mib(suggested_minimum_bytes)
        )
    } else if source_width > output_width || source_height > output_height {
        format!(
            "The video will be downscaled to {}×{} before the target bitrate is applied.",
            output_width, output_height
        )
    } else {
        "The original resolution will be retained and bitrate will be reduced to meet the size goal.".to_owned()
    };

    CompressionPlan {
        output_width,
        output_height,
        video_bps,
        predicted_output_bytes,
        quality_floor_bps,
        quality_label,
        message,
        compression_required,
    }
}

/// Converts the target size and duration into the available average video bitrate.
pub fn target_video_bitrate(duration_secs: f64, config: &JobConfig) -> u64 {
    let safe_duration = duration_secs.max(1.0);
    let target_bits =
        config.target_mib as f64 * MIB as f64 * 8.0 * (1.0 - config.size_margin.clamp(0.0, 0.25));
    let total_bps = target_bits / safe_duration;
    let audio_bps = config.audio_kbps as f64 * 1_000.0;
    (total_bps - audio_bps).max(MIN_VIDEO_BPS as f64) as u64
}

/// Chooses a lower bitrate for another attempt after an oversized output.
///
/// FFmpeg's actual output can differ from the arithmetic estimate, especially
/// with hardware encoders. The measured output size gives us a correction
/// factor for the next attempt.
pub fn retry_video_bitrate(
    previous_video_bps: u64,
    measured_output_bytes: u64,
    config: &JobConfig,
    quality_floor_bps: u64,
) -> Option<u64> {
    let target_bytes = config.target_mib.saturating_mul(MIB);
    if measured_output_bytes <= target_bytes {
        return None;
    }

    let correction = target_bytes as f64 / measured_output_bytes.max(1) as f64;
    let strategy_buffer = match config.quality_strategy {
        QualityStrategy::SmallestFile => 0.94,
        QualityStrategy::Balanced | QualityStrategy::FastestEncode => 0.97,
        QualityStrategy::BestQuality => 0.99,
    };
    let proposed = (previous_video_bps as f64 * correction * strategy_buffer) as u64;

    let minimum = if config.quality_strategy == QualityStrategy::BestQuality {
        quality_floor_bps.max(MIN_VIDEO_BPS)
    } else {
        MIN_VIDEO_BPS
    };
    let next = proposed.max(minimum);

    // Avoid an endless retry when the strategy's minimum bitrate prevents any
    // meaningful reduction.
    if next >= previous_video_bps.saturating_sub(10_000) {
        None
    } else {
        Some(next)
    }
}

/// Returns dimensions that fit inside the selected maximum while preserving
/// aspect ratio. Smaller sources are returned unchanged, so the app never
/// upscales video.
pub fn fit_without_upscaling(
    source_width: u32,
    source_height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    if source_width == 0 || source_height == 0 {
        return (max_width.max(2), max_height.max(2));
    }
    if source_width <= max_width && source_height <= max_height {
        return (
            make_even(source_width).max(2),
            make_even(source_height).max(2),
        );
    }

    let width_ratio = max_width as f64 / source_width as f64;
    let height_ratio = max_height as f64 / source_height as f64;
    let ratio = width_ratio.min(height_ratio).min(1.0);

    let width = make_even((source_width as f64 * ratio).floor() as u32);
    let height = make_even((source_height as f64 * ratio).floor() as u32);
    (width.max(2), height.max(2))
}

fn quality_floor(
    width: u32,
    height: u32,
    fps: f64,
    codec: Codec,
    strategy: QualityStrategy,
) -> u64 {
    let base_bits_per_pixel = match codec {
        Codec::H264 => 0.075,
        Codec::H265 => 0.052,
    };
    let strategy_multiplier = match strategy {
        QualityStrategy::BestQuality => 1.20,
        QualityStrategy::Balanced => 1.0,
        QualityStrategy::SmallestFile => 0.72,
        QualityStrategy::FastestEncode => 0.90,
    };
    let safe_fps = fps.clamp(12.0, 60.0);
    ((width as f64 * height as f64 * safe_fps * base_bits_per_pixel * strategy_multiplier) as u64)
        .max(MIN_VIDEO_BPS)
}

fn predicted_size(video_bps: u64, duration_secs: f64, audio_kbps: u32) -> u64 {
    let total_bps = video_bps.saturating_add(audio_kbps as u64 * 1_000);
    ((total_bps as f64 * duration_secs.max(1.0)) / 8.0) as u64
}

fn quality_label(ratio: f64) -> &'static str {
    if ratio >= 1.15 {
        "Excellent"
    } else if ratio >= 0.85 {
        "Good"
    } else if ratio >= 0.65 {
        "Fair"
    } else {
        "Low"
    }
}

fn make_even(value: u32) -> u32 {
    value.saturating_sub(value % 2)
}

fn bytes_to_mib(bytes: u64) -> u64 {
    ((bytes as f64 / MIB as f64).ceil()) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{EncoderMode, JobConfig};
    use std::path::PathBuf;

    fn config(strategy: QualityStrategy) -> JobConfig {
        JobConfig {
            input: PathBuf::from("input"),
            output: PathBuf::from("output"),
            include_subdirectories: true,
            target_mib: 1_000,
            max_width: 1_280,
            max_height: 720,
            codec: Codec::H265,
            encoder_mode: EncoderMode::Auto,
            software_preset: "veryfast".to_owned(),
            audio_kbps: 128,
            size_margin: 0.03,
            jobs: 2,
            overwrite: false,
            make_contact_sheet: true,
            skip_compliant: true,
            use_hardware: true,
            quality_strategy: strategy,
            retry_missed_target: true,
            max_encode_attempts: 2,
        }
    }

    #[test]
    fn never_upscales_small_video() {
        assert_eq!(fit_without_upscaling(640, 480, 1280, 720), (640, 480));
    }

    #[test]
    fn downscales_four_k_to_fit_720p() {
        assert_eq!(fit_without_upscaling(3840, 2160, 1280, 720), (1280, 720));
    }

    #[test]
    fn best_quality_can_exceed_size_bitrate() {
        let balanced = build_plan(
            3840,
            2160,
            10_800.0,
            24.0,
            8_000_000_000,
            &config(QualityStrategy::Balanced),
        );
        let quality = build_plan(
            3840,
            2160,
            10_800.0,
            24.0,
            8_000_000_000,
            &config(QualityStrategy::BestQuality),
        );
        assert!(quality.video_bps >= balanced.video_bps);
    }

    #[test]
    fn retry_reduces_bitrate_after_oversized_output() {
        let config = config(QualityStrategy::Balanced);
        let next = retry_video_bitrate(2_000_000, 1_300 * MIB, &config, 1_000_000).unwrap();
        assert!(next < 2_000_000);
    }
}
