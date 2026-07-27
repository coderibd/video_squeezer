//! Slint callback wiring.
//!
//! Keeping callbacks in one file makes the boundary between user actions and
//! background work easy to follow. Long-running work is always moved onto a
//! thread so the window remains responsive.

use crate::{
    app::{settings, view::refresh_ui},
    models::SharedState,
    scheduler::run_jobs,
    services::{build_plan, scan_videos},
    utils::show_message,
    AppWindow,
};
use slint::ComponentHandle;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::{atomic::Ordering, Arc},
    thread,
};

/// Connects every button and queue action exposed by `app.slint`.
pub fn wire(ui: &AppWindow, state: Arc<SharedState>) {
    wire_folder_buttons(ui);
    wire_scan(ui, state.clone());
    wire_start(ui, state.clone());
    wire_refresh_advice(ui, state.clone());
    wire_pause_and_stop(ui, state.clone());
    wire_selection(ui, state);
    wire_help(ui);
}

fn wire_folder_buttons(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_choose_input(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if let Some(ui) = weak.upgrade() {
                ui.set_input_path(folder.display().to_string().into());
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_choose_output(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            if let Some(ui) = weak.upgrade() {
                ui.set_output_path(folder.display().to_string().into());
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_open_output(move || {
        if let Some(ui) = weak.upgrade() {
            let path = ui.get_output_path().to_string();
            if !path.is_empty() {
                let _ = Command::new("open").arg(path).spawn();
            }
        }
    });
}

fn wire_scan(ui: &AppWindow, state: Arc<SharedState>) {
    let weak = ui.as_weak();
    ui.on_scan(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let input = PathBuf::from(ui.get_input_path().to_string());
        if input.as_os_str().is_empty() {
            show_message("Choose an input folder first.");
            return;
        }

        let include_subdirectories = ui.get_include_subdirectories();
        let advisor_config = settings::from_ui(&ui).ok();
        state.scanning.store(true, Ordering::SeqCst);
        refresh_ui(&weak, &state);

        let worker_ui = weak.clone();
        let worker_state = state.clone();
        thread::spawn(move || {
            let mut rows = scan_videos(&input, include_subdirectories);
            if let Some(config) = advisor_config.as_ref() {
                apply_advice(&mut rows, config);
            }
            *worker_state.rows.lock().expect("rows mutex") = rows;
            *worker_state.selected.lock().expect("selected mutex") = None;
            worker_state.scanning.store(false, Ordering::SeqCst);
            refresh_ui(&worker_ui, &worker_state);
        });
    });
}

fn wire_start(ui: &AppWindow, state: Arc<SharedState>) {
    let weak = ui.as_weak();
    ui.on_start(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };

        // `swap` both tests and sets the flag in one atomic operation, which
        // prevents a fast double-click from launching two schedulers.
        if state.running.swap(true, Ordering::SeqCst) {
            return;
        }
        state.cancel.store(false, Ordering::SeqCst);
        state.paused.store(false, Ordering::SeqCst);

        let config = match settings::from_ui(&ui) {
            Ok(config) => config,
            Err(error) => {
                state.running.store(false, Ordering::SeqCst);
                show_message(&error.to_string());
                return;
            }
        };

        let selected_jobs = config.jobs;
        ui.set_footer_center(
            format!(
                "Starting {selected_jobs} concurrent worker{}",
                if selected_jobs == 1 { "" } else { "s" }
            )
            .into(),
        );

        let worker_ui = weak.clone();
        let worker_state = state.clone();
        thread::spawn(move || {
            if worker_state.rows.lock().expect("rows mutex").is_empty() {
                worker_state.scanning.store(true, Ordering::SeqCst);
                refresh_ui(&worker_ui, &worker_state);
                let rows = scan_videos(&config.input, config.include_subdirectories);
                *worker_state.rows.lock().expect("rows mutex") = rows;
                worker_state.scanning.store(false, Ordering::SeqCst);
                refresh_ui(&worker_ui, &worker_state);
            }

            if let Err(error) = fs::create_dir_all(&config.output) {
                worker_state.running.store(false, Ordering::SeqCst);
                show_message(&format!("Unable to create output folder: {error}"));
                refresh_ui(&worker_ui, &worker_state);
                return;
            }

            {
                let mut rows = worker_state.rows.lock().expect("rows mutex");
                apply_advice(&mut rows, &config);
            }
            refresh_ui(&worker_ui, &worker_state);
            run_jobs(config, worker_state.clone(), worker_ui.clone());
            worker_state.running.store(false, Ordering::SeqCst);
            worker_state.paused.store(false, Ordering::SeqCst);
            refresh_ui(&worker_ui, &worker_state);
        });
    });
}

/// Recalculates estimates after the user changes size, resolution, codec, or strategy.
fn wire_refresh_advice(ui: &AppWindow, state: Arc<SharedState>) {
    let weak = ui.as_weak();
    ui.on_refresh_advice(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let config = match settings::from_ui(&ui) {
            Ok(config) => config,
            Err(error) => {
                show_message(&error.to_string());
                return;
            }
        };

        {
            let mut rows = state.rows.lock().expect("rows mutex");
            apply_advice(&mut rows, &config);
        }
        refresh_ui(&weak, &state);
    });
}

/// Applies the Compression Advisor to every queue row without starting FFmpeg.
fn apply_advice(rows: &mut [crate::models::VideoRow], config: &crate::models::JobConfig) {
    for row in rows {
        let plan = build_plan(
            row.width,
            row.height,
            row.duration_secs,
            row.fps,
            row.original_bytes,
            config,
        );
        row.predicted_output_bytes = Some(plan.predicted_output_bytes);
        row.planned_width = Some(plan.output_width);
        row.planned_height = Some(plan.output_height);
        row.recommended_video_bps = Some(plan.video_bps);
        row.quality_label = plan.quality_label.to_owned();
        row.advisor_message = plan.message;
    }
}

fn wire_pause_and_stop(ui: &AppWindow, state: Arc<SharedState>) {
    let weak = ui.as_weak();
    let pause_state = state.clone();
    ui.on_pause(move || {
        let new_value = !pause_state.paused.load(Ordering::SeqCst);
        pause_state.paused.store(new_value, Ordering::SeqCst);
        refresh_ui(&weak, &pause_state);
    });

    let weak = ui.as_weak();
    ui.on_stop(move || {
        state.cancel.store(true, Ordering::SeqCst);
        refresh_ui(&weak, &state);
    });
}

fn wire_selection(ui: &AppWindow, state: Arc<SharedState>) {
    let weak = ui.as_weak();
    ui.on_select_row(move |index| {
        *state.selected.lock().expect("selected mutex") = Some(index.max(0) as usize);
        refresh_ui(&weak, &state);
    });
}

fn wire_help(ui: &AppWindow) {
    ui.on_show_help(move |topic| {
        let text = match topic.as_str() {
            "codec" => "H.264 offers the broadest playback compatibility. H.265 usually produces smaller files at similar quality.",
            "encoder" => "Auto uses Apple VideoToolbox when available and falls back to software encoding.",
            "preset" => "Faster presets reduce encoding time. Slower presets improve compression efficiency but take longer.",
            "audio" => "128 kbps AAC is a good default for most video. Higher values preserve more audio detail but reduce the video bitrate budget.",
            "strategy" => "Balanced targets the requested size. Best Quality protects a calculated bitrate floor and may exceed the target. Smallest File uses a larger safety buffer. Fastest Encode favors hardware and faster software presets.",
            _ => "",
        };
        show_message(text);
    });
}
