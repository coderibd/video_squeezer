//! FFmpeg encoder selection.

use crate::models::{Codec, EncoderMode, JobConfig};
use anyhow::Result;
use std::process::Command;

/// Selects the exact FFmpeg encoder name for the requested codec and mode.
///
/// In Auto mode, VideoToolbox is preferred when the installed FFmpeg supports
/// the matching hardware encoder. Software encoding is used as the fallback.
pub fn select_encoder(config: &JobConfig) -> Result<String> {
    let hardware_available = ffmpeg_has_encoder(hardware_encoder(config.codec));
    select_encoder_name(config, hardware_available).map(str::to_owned)
}

/// Makes the encoder-selection decision without launching an external process.
///
/// Keeping this decision separate from FFmpeg capability detection makes the
/// policy easy to unit test. The public `select_encoder` function supplies the
/// real capability result discovered from the installed FFmpeg executable.
fn select_encoder_name(config: &JobConfig, hardware_available: bool) -> Result<&'static str> {
    let software = software_encoder(config.codec);
    let hardware = hardware_encoder(config.codec);

    if !config.use_hardware || config.encoder_mode == EncoderMode::Software {
        return Ok(software);
    }

    match config.encoder_mode {
        EncoderMode::Software => Ok(software),
        EncoderMode::VideoToolbox => {
            anyhow::ensure!(hardware_available, "FFmpeg does not provide {hardware}");
            Ok(hardware)
        }
        EncoderMode::Auto => Ok(if hardware_available {
            hardware
        } else {
            software
        }),
    }
}

/// Returns the software encoder corresponding to the requested codec.
fn software_encoder(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "libx264",
        Codec::H265 => "libx265",
    }
}

/// Returns the Apple VideoToolbox encoder corresponding to the requested codec.
fn hardware_encoder(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h264_videotoolbox",
        Codec::H265 => "hevc_videotoolbox",
    }
}

/// Checks the encoder list exposed by the installed FFmpeg executable.
fn ffmpeg_has_encoder(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(name))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::QualityStrategy;
    use std::path::PathBuf;

    fn config(codec: Codec, mode: EncoderMode, use_hardware: bool) -> JobConfig {
        JobConfig {
            input: PathBuf::from("input"),
            output: PathBuf::from("output"),
            include_subdirectories: true,
            target_mib: 1_000,
            max_width: 1_280,
            max_height: 720,
            codec,
            encoder_mode: mode,
            software_preset: "veryfast".to_owned(),
            audio_kbps: 128,
            size_margin: 0.03,
            jobs: 2,
            overwrite: false,
            make_contact_sheet: true,
            skip_compliant: true,
            use_hardware,
            quality_strategy: QualityStrategy::Balanced,
            retry_missed_target: true,
            max_encode_attempts: 2,
        }
    }

    #[test]
    fn auto_prefers_h264_videotoolbox_when_available() {
        let config = config(Codec::H264, EncoderMode::Auto, true);
        assert_eq!(
            select_encoder_name(&config, true).unwrap(),
            "h264_videotoolbox"
        );
    }

    #[test]
    fn auto_falls_back_to_software_when_hardware_is_missing() {
        let config = config(Codec::H265, EncoderMode::Auto, true);
        assert_eq!(select_encoder_name(&config, false).unwrap(), "libx265");
    }

    #[test]
    fn hardware_toggle_can_force_software() {
        let config = config(Codec::H264, EncoderMode::Auto, false);
        assert_eq!(select_encoder_name(&config, true).unwrap(), "libx264");
    }

    #[test]
    fn forced_videotoolbox_reports_missing_encoder() {
        let config = config(Codec::H265, EncoderMode::VideoToolbox, true);
        let error = select_encoder_name(&config, false).unwrap_err();
        assert!(error.to_string().contains("hevc_videotoolbox"));
    }
}
