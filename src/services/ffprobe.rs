//! FFprobe wrapper and JSON parsing.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{path::Path, process::Command};

/// Metadata required by the size calculation and user interface.
#[derive(Debug, Clone)]
pub struct ProbeInfo {
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub fps: f64,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Runs FFprobe and returns dimensions, duration, and frame rate for the first
/// video stream.
pub fn probe_video(path: &Path) -> Result<ProbeInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_streams",
            "-show_format",
        ])
        .arg(path)
        .output()?;

    anyhow::ensure!(output.status.success(), "ffprobe failed");

    let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)?;
    let stream = parsed
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .context("no video stream found")?;

    let duration = stream
        .duration
        .as_deref()
        .or(parsed.format.duration.as_deref())
        .context("duration is unavailable")?
        .parse::<f64>()?;

    anyhow::ensure!(duration.is_finite() && duration > 0.0, "invalid duration");

    let fps = stream
        .avg_frame_rate
        .as_deref()
        .or(stream.r_frame_rate.as_deref())
        .and_then(parse_fraction)
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(30.0);

    Ok(ProbeInfo {
        width: stream.width.unwrap_or(0),
        height: stream.height.unwrap_or(0),
        duration_secs: duration,
        fps,
    })
}

/// Parses FFprobe values such as `24000/1001` or `30/1`.
fn parse_fraction(value: &str) -> Option<f64> {
    let (numerator, denominator) = value.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    if denominator == 0.0 {
        None
    } else {
        Some(numerator / denominator)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_fraction;

    #[test]
    fn parses_common_ntsc_frame_rate() {
        let fps = parse_fraction("24000/1001").unwrap();
        assert!((fps - 23.976).abs() < 0.01);
    }

    #[test]
    fn rejects_zero_denominator() {
        assert!(parse_fraction("30/0").is_none());
    }
}
