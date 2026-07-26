//! Processing logic for one queue item.

use crate::{
    app::view::refresh_ui,
    models::{Codec, FileState, JobConfig, SharedState, VideoRow},
    services::{create_contact_sheet, create_preview_frame, probe_video, select_encoder},
    utils::sanitize_filename,
    AppWindow,
};
use anyhow::{Context, Result};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::Path,
    process::{Command, Stdio},
    sync::{atomic::Ordering, Arc},
    thread,
    time::{Duration, Instant},
};

const MIB: u64 = 1024 * 1024;

/// Processes one queue row from start to finish.
///
/// Important safety behavior:
/// - The source file is never modified.
/// - FFmpeg writes to a hidden partial file.
/// - The partial file is renamed only after FFmpeg succeeds.
/// - Cancellation removes the partial output.
pub fn process_video(
    index: usize,
    config: &JobConfig,
    state: &Arc<SharedState>,
    weak: &slint::Weak<AppWindow>,
) -> Result<()> {
    wait_if_paused(state);
    if state.cancel.load(Ordering::SeqCst) {
        mark_cancelled(index, state, weak);
        return Ok(());
    }

    let row = state.rows.lock().expect("rows mutex")[index].clone();
    let probe = probe_video(&row.path)?;
    let must_compress = row.original_bytes > config.target_mib * MIB
        || probe.width > config.max_width
        || probe.height > config.max_height;

    let relative = row.path.strip_prefix(&config.input).unwrap_or(&row.path);
    let output_dir = config
        .output
        .join(relative.parent().unwrap_or_else(|| Path::new("")));
    fs::create_dir_all(&output_dir)?;

    let stem = row
        .path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("video");
    let output_path = output_dir.join(format!("{stem}.compressed.mp4"));
    let partial_path = output_dir.join(format!(".{stem}.compressed.partial.mp4"));
    let contact_path = output_dir.join(format!("{stem}.contact-sheet.jpg"));
    let contact_partial = output_dir.join(format!(".{stem}.contact-sheet.partial.jpg"));
    let preview_dir = config.output.join(".video-squeezer-previews");
    fs::create_dir_all(&preview_dir)?;
    let preview_path = preview_dir.join(format!("{}-{}.jpg", index, sanitize_filename(stem)));

    // A failed preview is non-fatal. Encoding should still continue.
    if create_preview_frame(&row.path, &preview_path, probe.duration_secs).is_ok() {
        update_row(index, state, |row| {
            row.preview_path = Some(preview_path.clone())
        });
        refresh_ui(weak, state);
    }

    if !must_compress && config.skip_compliant {
        if config.make_contact_sheet && (config.overwrite || !contact_path.exists()) {
            create_contact_sheet(&row.path, &contact_partial, &contact_path)?;
        }
        update_row(index, state, |row| {
            row.state = FileState::Skipped;
            row.progress = 1.0;
            row.message = "Already within configured limits".to_owned();
        });
        refresh_ui(weak, state);
        return Ok(());
    }

    if output_path.exists() && !config.overwrite {
        let size = fs::metadata(&output_path)?.len();
        update_row(index, state, |row| {
            row.state = FileState::Completed;
            row.progress = 1.0;
            row.output_bytes = Some(size);
            row.message = "Existing output retained".to_owned();
        });
        refresh_ui(weak, state);
        return Ok(());
    }

    let encoder = select_encoder(config)?;
    update_row(index, state, |row| {
        row.state = FileState::Processing;
        row.encoder = Some(encoder.clone());
        row.started_at = Some(Instant::now());
        row.progress = 0.0;
    });
    refresh_ui(weak, state);

    // Convert the requested maximum file size into an average bitrate budget.
    // Audio is subtracted first; the remaining bits are assigned to video.
    let target_bits =
        (config.target_mib as f64 * MIB as f64 * 8.0 * (1.0 - config.size_margin)) as u64;
    let audio_bps = config.audio_kbps as u64 * 1000;
    let video_bps = ((target_bits as f64 / probe.duration_secs) as u64)
        .saturating_sub(audio_bps)
        .max(250_000);
    let scale = format!(
        "scale={}:{}:force_original_aspect_ratio=decrease:force_divisible_by=2",
        config.max_width, config.max_height
    );

    let _ = fs::remove_file(&partial_path);
    let mut command = Command::new("ffmpeg");
    command
        .args(["-hide_banner", "-y", "-i"])
        .arg(&row.path)
        .args([
            "-map", "0:v:0", "-map", "0:a?", "-vf", &scale, "-c:v", &encoder,
        ]);

    if encoder.contains("videotoolbox") {
        command.args(["-b:v", &video_bps.to_string(), "-allow_sw", "1"]);
        if config.codec == Codec::H265 {
            command.args(["-tag:v", "hvc1"]);
        }
    } else {
        command.args([
            "-preset",
            &config.software_preset,
            "-b:v",
            &video_bps.to_string(),
        ]);
    }

    command
        .args(["-c:a", "aac", "-b:a", &format!("{}k", config.audio_kbps)])
        .args(["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"])
        .arg(&partial_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().context("unable to start ffmpeg")?;
    let stdout = child
        .stdout
        .take()
        .context("unable to read ffmpeg progress")?;
    let reader = BufReader::new(stdout);
    let mut last_progress = -1.0_f32;

    for line in reader.lines().map_while(Result::ok) {
        wait_if_paused(state);

        if state.cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&partial_path);
            mark_cancelled(index, state, weak);
            return Ok(());
        }

        if let Some(value) = line.strip_prefix("out_time_ms=") {
            if let Ok(microseconds) = value.parse::<f64>() {
                let progress =
                    (microseconds / 1_000_000.0 / probe.duration_secs).clamp(0.0, 1.0) as f32;

                // Limit GUI updates to roughly every half percent so a busy
                // worker pool does not overwhelm the event loop.
                if progress - last_progress >= 0.005 || progress >= 1.0 {
                    let elapsed = state.rows.lock().expect("rows mutex")[index]
                        .started_at
                        .map(|value| value.elapsed().as_secs_f64())
                        .unwrap_or_default();
                    let speed = if elapsed > 0.0 {
                        progress as f64 * probe.duration_secs / elapsed
                    } else {
                        0.0
                    };

                    update_row(index, state, |row| {
                        row.progress = progress;
                        row.speed = speed;
                    });
                    refresh_ui(weak, state);
                    last_progress = progress;
                }
            }
        }
    }

    let status = child.wait()?;
    anyhow::ensure!(status.success(), "FFmpeg exited with {status}");

    if output_path.exists() {
        fs::remove_file(&output_path)?;
    }
    fs::rename(&partial_path, &output_path)?;

    if config.make_contact_sheet {
        create_contact_sheet(&output_path, &contact_partial, &contact_path)?;
    }

    let output_bytes = fs::metadata(&output_path)?.len();
    update_row(index, state, |row| {
        row.state = FileState::Completed;
        row.progress = 1.0;
        row.output_bytes = Some(output_bytes);
        row.message = format!("Encoded with {encoder}");
    });
    refresh_ui(weak, state);
    Ok(())
}

/// Blocks a worker while the global pause flag is active.
fn wait_if_paused(state: &SharedState) {
    while state.paused.load(Ordering::SeqCst) && !state.cancel.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(150));
    }
}

/// Moves a row into the cancelled state and refreshes the interface.
pub fn mark_cancelled(index: usize, state: &Arc<SharedState>, weak: &slint::Weak<AppWindow>) {
    update_row(index, state, |row| {
        row.state = FileState::Cancelled;
        row.message = "Cancelled".to_owned();
    });
    refresh_ui(weak, state);
}

/// Applies one small mutation to a row while holding the rows mutex briefly.
pub fn update_row(index: usize, state: &SharedState, update: impl FnOnce(&mut VideoRow)) {
    if let Some(row) = state.rows.lock().expect("rows mutex").get_mut(index) {
        update(row);
    }
}
