//! Preview frame and contact-sheet generation.

use anyhow::Result;
use std::{fs, path::Path, process::Command};

/// Extracts a single lightweight JPEG used by the details panel.
pub fn create_preview_frame(input: &Path, output: &Path, duration_secs: f64) -> Result<()> {
    let seek = (duration_secs * 0.10).clamp(1.0, 300.0);
    let status = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-ss",
            &format!("{seek:.3}"),
            "-i",
        ])
        .arg(input)
        .args(["-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "3"])
        .arg(output)
        .status()?;

    anyhow::ensure!(status.success(), "unable to create preview frame");
    Ok(())
}

/// Generates a 4-by-3 collage without relying on FFmpeg's optional drawtext filter.
pub fn create_contact_sheet(input: &Path, partial: &Path, final_path: &Path) -> Result<()> {
    let _ = fs::remove_file(partial);
    let filter = "fps=1/60,scale=320:-2,tile=4x3:nb_frames=12:padding=4:margin=4";
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vf", filter, "-frames:v", "1", "-f", "image2"])
        .arg(partial)
        .status()?;

    anyhow::ensure!(
        status.success(),
        "FFmpeg failed while creating the contact sheet"
    );

    if final_path.exists() {
        fs::remove_file(final_path)?;
    }
    fs::rename(partial, final_path)?;
    Ok(())
}
