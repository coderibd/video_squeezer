//! Projects Rust state into Slint properties.
//!
//! Worker threads never manipulate widgets directly. They call this function,
//! which clones a small state snapshot and schedules the actual UI updates on
//! Slint's event-loop thread.

use crate::{
    models::{FileState, SharedState},
    utils::{format_bytes, format_duration},
    AppWindow, QueueItem,
};
use slint::{Image, ModelRc, SharedString, VecModel};
use std::sync::{atomic::Ordering, Arc};

/// Refreshes all queue, summary, footer, and selected-file properties.
pub fn refresh_ui(weak: &slint::Weak<AppWindow>, state: &Arc<SharedState>) {
    let rows = state.rows.lock().expect("rows mutex").clone();
    let selected = *state.selected.lock().expect("selected mutex");
    let running = state.running.load(Ordering::SeqCst);
    let paused = state.paused.load(Ordering::SeqCst);
    let scanning = state.scanning.load(Ordering::SeqCst);
    let selected_row = rows.get(selected.unwrap_or(usize::MAX)).cloned();

    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };

            let queue_items: Vec<QueueItem> = rows
                .iter()
                .map(|row| QueueItem {
                    name: SharedString::from(row.name()),
                    status: SharedString::from(row.state.label()),
                    status_icon: SharedString::from(row.state.icon()),
                    status_color: row.state.color(),
                    progress: row.progress,
                    original: SharedString::from(format_bytes(row.original_bytes)),
                    output: SharedString::from(
                        row.output_bytes
                            .map(format_bytes)
                            .or_else(|| {
                                row.predicted_output_bytes
                                    .map(|value| format!("~{}", format_bytes(value)))
                            })
                            .unwrap_or_else(|| "—".to_owned()),
                    ),
                    resolution: SharedString::from(match (row.planned_width, row.planned_height) {
                        (Some(width), Some(height))
                            if width != row.width || height != row.height =>
                        {
                            format!("{}×{} → {}×{}", row.width, row.height, width, height)
                        }
                        _ => format!("{}×{}", row.width, row.height),
                    }),
                })
                .collect();
            ui.set_queue(ModelRc::new(VecModel::from(queue_items)));

            let total = rows.len() as i32;
            let completed = rows
                .iter()
                .filter(|row| matches!(row.state, FileState::Completed | FileState::Skipped))
                .count() as i32;
            let processing = rows
                .iter()
                .filter(|row| row.state == FileState::Processing)
                .count() as i32;
            let remaining = rows
                .iter()
                .filter(|row| row.state == FileState::Queued)
                .count() as i32;
            let overall = if rows.is_empty() {
                0.0
            } else {
                rows.iter().map(|row| row.progress as f64).sum::<f64>() / rows.len() as f64
            };

            ui.set_total_files(total);
            ui.set_completed_files(completed);
            ui.set_processing_files(processing);
            ui.set_remaining_files(remaining);
            ui.set_overall_progress(overall as f32);
            ui.set_running(running);
            ui.set_paused(paused);
            ui.set_scanning(scanning);

            ui.set_footer_status(
                if scanning {
                    "Loading queue"
                } else if running {
                    if paused {
                        "Encoding paused"
                    } else {
                        "VideoToolbox hardware encoding active"
                    }
                } else {
                    "Ready"
                }
                .into(),
            );

            ui.set_footer_center(if scanning {
                "Reading video metadata…".into()
            } else if processing == 1 {
                "1 file processing".into()
            } else {
                format!("{processing} files processing").into()
            });

            if let Some(row) = selected_row {
                ui.set_selected_name(row.name().into());
                ui.set_selected_metadata(
                    format!(
                        "{}×{}   •   {:.2} fps   •   {}   •   {}",
                        row.width,
                        row.height,
                        row.fps,
                        format_duration(row.duration_secs),
                        format_bytes(row.original_bytes)
                    )
                    .into(),
                );

                let encoder = row
                    .encoder
                    .clone()
                    .unwrap_or_else(|| "Awaiting encoder selection".to_owned());
                ui.set_selected_encoder(
                    if encoder.contains("videotoolbox") {
                        format!("Encoding with {encoder} (Hardware)")
                    } else {
                        format!("Encoding with {encoder}")
                    }
                    .into(),
                );

                let elapsed = row
                    .started_at
                    .map(|value| value.elapsed().as_secs_f64())
                    .unwrap_or_default();
                let remaining_time = if row.progress > 0.0 && elapsed > 0.0 {
                    elapsed * (1.0 / row.progress as f64 - 1.0)
                } else {
                    0.0
                };

                ui.set_selected_eta(format_duration(remaining_time).into());
                ui.set_selected_elapsed(format_duration(elapsed).into());
                ui.set_selected_speed(format!("{:.2}×", row.speed).into());

                let compression = row
                    .output_bytes
                    .map(|value| 100.0 * (1.0 - value as f64 / row.original_bytes.max(1) as f64))
                    .unwrap_or(0.0);
                ui.set_selected_compression(format!("{compression:.0}%").into());
                ui.set_selected_progress(row.progress);
                ui.set_selected_quality(if row.quality_label.is_empty() {
                    "Not analyzed".into()
                } else {
                    row.quality_label.clone().into()
                });
                ui.set_selected_predicted_size(
                    row.predicted_output_bytes
                        .map(|value| format!("Estimated output: {}", format_bytes(value)))
                        .unwrap_or_else(|| "Estimated output unavailable".to_owned())
                        .into(),
                );
                ui.set_selected_target_bitrate(
                    row.recommended_video_bps
                        .map(|value| {
                            format!(
                                "Target video bitrate: {:.2} Mbps",
                                value as f64 / 1_000_000.0
                            )
                        })
                        .unwrap_or_else(|| "Target bitrate unavailable".to_owned())
                        .into(),
                );
                ui.set_selected_advice(if row.advisor_message.is_empty() {
                    "Choose settings and refresh estimates to see compression advice.".into()
                } else {
                    row.advisor_message.clone().into()
                });

                if let Some(path) = row.preview_path {
                    if let Ok(image) = Image::load_from_path(&path) {
                        ui.set_preview_image(image);
                    }
                }
            }
        }
    });
}
