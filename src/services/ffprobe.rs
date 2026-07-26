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
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

/// Runs FFprobe and returns dimensions and duration for the first video stream.
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

    Ok(ProbeInfo {
        width: stream.width.unwrap_or(0),
        height: stream.height.unwrap_or(0),
        duration_secs: duration,
    })
}
