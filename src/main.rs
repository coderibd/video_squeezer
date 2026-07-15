use anyhow::{Context, Result};
use serde::Deserialize;
use slint::{Color, Image, ModelRc, SharedString, VecModel};
use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

slint::include_modules!();

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    H264,
    H265,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EncoderMode {
    Auto,
    VideoToolbox,
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileState {
    Queued,
    Processing,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

impl FileState {
    fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Processing => "Processing",
            Self::Completed => "Completed",
            Self::Skipped => "Skipped",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            Self::Queued => "◷",
            Self::Processing => "▶",
            Self::Completed => "✓",
            Self::Skipped => "↷",
            Self::Failed => "!",
            Self::Cancelled => "■",
        }
    }

    fn color(&self) -> Color {
        match self {
            Self::Queued => Color::from_rgb_u8(132, 139, 150),
            Self::Processing => Color::from_rgb_u8(38, 112, 238),
            Self::Completed => Color::from_rgb_u8(49, 169, 82),
            Self::Skipped => Color::from_rgb_u8(128, 76, 204),
            Self::Failed => Color::from_rgb_u8(211, 66, 66),
            Self::Cancelled => Color::from_rgb_u8(235, 144, 24),
        }
    }
}

#[derive(Debug, Clone)]
struct VideoRow {
    path: PathBuf,
    state: FileState,
    progress: f32,
    original_bytes: u64,
    output_bytes: Option<u64>,
    width: u32,
    height: u32,
    duration_secs: f64,
    preview_path: Option<PathBuf>,
    encoder: Option<String>,
    started_at: Option<Instant>,
    speed: f64,
    message: String,
}

impl VideoRow {
    fn name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>")
            .to_owned()
    }
}

#[derive(Clone)]
struct JobConfig {
    input: PathBuf,
    output: PathBuf,
    include_subdirectories: bool,
    target_mib: u64,
    max_width: u32,
    max_height: u32,
    codec: Codec,
    encoder_mode: EncoderMode,
    software_preset: String,
    audio_kbps: u32,
    size_margin: f64,
    jobs: usize,
    overwrite: bool,
    make_contact_sheet: bool,
    skip_compliant: bool,
    use_hardware: bool,
}

#[derive(Default)]
struct SharedState {
    rows: Mutex<Vec<VideoRow>>,
    selected: Mutex<Option<usize>>,
    cancel: AtomicBool,
    paused: AtomicBool,
    running: AtomicBool,
}

fn main() -> Result<()> {
    let ui = AppWindow::new()?;
    let state = Arc::new(SharedState::default());

    wire_callbacks(&ui, state.clone());
    refresh_ui(&ui.as_weak(), &state);
    ui.run()?;
    Ok(())
}

fn wire_callbacks(ui: &AppWindow, state: Arc<SharedState>) {
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

    let weak = ui.as_weak();
    let scan_state = state.clone();
    ui.on_scan(move || {
        let Some(ui) = weak.upgrade() else { return };
        let input = PathBuf::from(ui.get_input_path().to_string());
        if input.as_os_str().is_empty() {
            show_message("Choose an input folder first.");
            return;
        }
        let include = ui.get_include_subdirectories();
        let weak2 = weak.clone();
        let state2 = scan_state.clone();
        thread::spawn(move || {
            let rows = scan_videos(&input, include);
            *state2.rows.lock().expect("rows mutex") = rows;
            *state2.selected.lock().expect("selected mutex") = None;
            refresh_ui(&weak2, &state2);
        });
    });

    let weak = ui.as_weak();
    let start_state = state.clone();
    ui.on_start(move || {
        let Some(ui) = weak.upgrade() else { return };
        if start_state.running.swap(true, Ordering::SeqCst) {
            return;
        }
        start_state.cancel.store(false, Ordering::SeqCst);
        start_state.paused.store(false, Ordering::SeqCst);

        let config = match config_from_ui(&ui) {
            Ok(config) => config,
            Err(error) => {
                start_state.running.store(false, Ordering::SeqCst);
                show_message(&error.to_string());
                return;
            }
        };

        let weak2 = weak.clone();
        let state2 = start_state.clone();
        thread::spawn(move || {
            if state2.rows.lock().expect("rows mutex").is_empty() {
                let rows = scan_videos(&config.input, config.include_subdirectories);
                *state2.rows.lock().expect("rows mutex") = rows;
                refresh_ui(&weak2, &state2);
            }

            if let Err(error) = fs::create_dir_all(&config.output) {
                state2.running.store(false, Ordering::SeqCst);
                show_message(&format!("Unable to create output folder: {error}"));
                refresh_ui(&weak2, &state2);
                return;
            }

            run_jobs(config, state2.clone(), weak2.clone());
            state2.running.store(false, Ordering::SeqCst);
            state2.paused.store(false, Ordering::SeqCst);
            refresh_ui(&weak2, &state2);
        });
    });

    let weak = ui.as_weak();
    let pause_state = state.clone();
    ui.on_pause(move || {
        let new_value = !pause_state.paused.load(Ordering::SeqCst);
        pause_state.paused.store(new_value, Ordering::SeqCst);
        refresh_ui(&weak, &pause_state);
    });

    let weak = ui.as_weak();
    let stop_state = state.clone();
    ui.on_stop(move || {
        stop_state.cancel.store(true, Ordering::SeqCst);
        refresh_ui(&weak, &stop_state);
    });

    let weak = ui.as_weak();
    let select_state = state.clone();
    ui.on_select_row(move |index| {
        *select_state.selected.lock().expect("selected mutex") = Some(index.max(0) as usize);
        refresh_ui(&weak, &select_state);
    });

    ui.on_show_help(move |topic| {
        let text = match topic.as_str() {
            "codec" => "H.264 offers the broadest playback compatibility. H.265 usually produces smaller files at similar quality.",
            "encoder" => "Auto uses Apple VideoToolbox when available and falls back to software encoding.",
            "preset" => "Faster presets reduce encoding time. Slower presets improve compression efficiency but take longer.",
            "audio" => "128 kbps AAC is a good default for most video. Higher values preserve more audio detail but reduce the video bitrate budget.",
            _ => "",
        };
        show_message(text);
    });
}

fn config_from_ui(ui: &AppWindow) -> Result<JobConfig> {
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

    let codec = if ui.get_codec().contains("264") { Codec::H264 } else { Codec::H265 };
    let encoder_mode = match ui.get_encoder().as_str() {
        value if value.starts_with("Apple") => EncoderMode::VideoToolbox,
        value if value.starts_with("Software") => EncoderMode::Software,
        _ => EncoderMode::Auto,
    };
    let software_preset = ui.get_preset().to_string().to_ascii_lowercase().replace(' ', "");
    let audio_kbps = ui
        .get_audio_bitrate()
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(128);

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
        jobs: ui.get_jobs().clamp(1, 8) as usize,
        overwrite: ui.get_overwrite_existing(),
        make_contact_sheet: ui.get_generate_contact_sheet(),
        skip_compliant: ui.get_skip_compliant(),
        use_hardware: ui.get_use_hardware(),
    })
}

fn run_jobs(config: JobConfig, state: Arc<SharedState>, weak: slint::Weak<AppWindow>) {
    let next = Arc::new(AtomicUsize::new(0));
    let row_count = state.rows.lock().expect("rows mutex").len();
    let mut workers = Vec::new();

    for _ in 0..config.jobs.min(row_count.max(1)) {
        let config = config.clone();
        let state = state.clone();
        let weak = weak.clone();
        let next = next.clone();
        workers.push(thread::spawn(move || loop {
            let index = next.fetch_add(1, Ordering::SeqCst);
            if index >= state.rows.lock().expect("rows mutex").len() {
                break;
            }
            if state.cancel.load(Ordering::SeqCst) {
                mark_cancelled(index, &state, &weak);
                continue;
            }
            if let Err(error) = process_video(index, &config, &state, &weak) {
                update_row(index, &state, |row| {
                    row.state = FileState::Failed;
                    row.message = format!("{error:#}");
                });
                refresh_ui(&weak, &state);
            }
        }));
    }

    for worker in workers {
        let _ = worker.join();
    }
}

fn process_video(index: usize, config: &JobConfig, state: &Arc<SharedState>, weak: &slint::Weak<AppWindow>) -> Result<()> {
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
    let output_dir = config.output.join(relative.parent().unwrap_or_else(|| Path::new("")));
    fs::create_dir_all(&output_dir)?;
    let stem = row.path.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let output_path = output_dir.join(format!("{stem}.compressed.mp4"));
    let partial_path = output_dir.join(format!(".{stem}.compressed.partial.mp4"));
    let contact_path = output_dir.join(format!("{stem}.contact-sheet.jpg"));
    let contact_partial = output_dir.join(format!(".{stem}.contact-sheet.partial.jpg"));
    let preview_dir = config.output.join(".video-squeezer-previews");
    fs::create_dir_all(&preview_dir)?;
    let preview_path = preview_dir.join(format!("{}-{}.jpg", index, sanitize_filename(stem)));

    if create_preview_frame(&row.path, &preview_path, probe.duration_secs).is_ok() {
        update_row(index, state, |row| row.preview_path = Some(preview_path.clone()));
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

    let target_bits = (config.target_mib as f64 * MIB as f64 * 8.0 * (1.0 - config.size_margin)) as u64;
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
        .args(["-map", "0:v:0", "-map", "0:a?", "-vf", &scale, "-c:v", &encoder]);

    if encoder.contains("videotoolbox") {
        command.args(["-b:v", &video_bps.to_string(), "-allow_sw", "1"]);
        if config.codec == Codec::H265 {
            command.args(["-tag:v", "hvc1"]);
        }
    } else {
        command.args(["-preset", &config.software_preset, "-b:v", &video_bps.to_string()]);
    }

    command
        .args(["-c:a", "aac", "-b:a", &format!("{}k", config.audio_kbps)])
        .args(["-movflags", "+faststart", "-progress", "pipe:1", "-nostats"])
        .arg(&partial_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = command.spawn().context("unable to start ffmpeg")?;
    let stdout = child.stdout.take().context("unable to read ffmpeg progress")?;
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
                let progress = (microseconds / 1_000_000.0 / probe.duration_secs).clamp(0.0, 1.0) as f32;
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

fn wait_if_paused(state: &SharedState) {
    while state.paused.load(Ordering::SeqCst) && !state.cancel.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(150));
    }
}

fn mark_cancelled(index: usize, state: &Arc<SharedState>, weak: &slint::Weak<AppWindow>) {
    update_row(index, state, |row| {
        row.state = FileState::Cancelled;
        row.message = "Cancelled".to_owned();
    });
    refresh_ui(weak, state);
}

fn update_row(index: usize, state: &SharedState, f: impl FnOnce(&mut VideoRow)) {
    if let Some(row) = state.rows.lock().expect("rows mutex").get_mut(index) {
        f(row);
    }
}

fn refresh_ui(weak: &slint::Weak<AppWindow>, state: &Arc<SharedState>) {
    let rows = state.rows.lock().expect("rows mutex").clone();
    let selected = *state.selected.lock().expect("selected mutex");
    let running = state.running.load(Ordering::SeqCst);
    let paused = state.paused.load(Ordering::SeqCst);
    let state_for_image = rows.get(selected.unwrap_or(usize::MAX)).cloned();

    let _ = slint::invoke_from_event_loop({
        let weak = weak.clone();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let queue_items: Vec<QueueItem> = rows
                .iter()
                .map(|row| QueueItem {
                    name: SharedString::from(row.name()),
                    status: SharedString::from(row.state.label()),
                    status_icon: SharedString::from(row.state.icon()),
                    status_color: row.state.color(),
                    progress: row.progress,
                    original: SharedString::from(format_bytes(row.original_bytes)),
                    output: SharedString::from(row.output_bytes.map(format_bytes).unwrap_or_else(|| "—".to_owned())),
                    resolution: SharedString::from(format!("{}×{}", row.width, row.height)),
                })
                .collect();
            ui.set_queue(ModelRc::new(VecModel::from(queue_items)));

            let total = rows.len() as i32;
            let completed = rows
                .iter()
                .filter(|row| matches!(row.state, FileState::Completed | FileState::Skipped))
                .count() as i32;
            let processing = rows.iter().filter(|row| row.state == FileState::Processing).count() as i32;
            let remaining = rows.iter().filter(|row| row.state == FileState::Queued).count() as i32;
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
            ui.set_footer_status(
                if running {
                    if paused { "Encoding paused" } else { "VideoToolbox hardware encoding active" }
                } else {
                    "Ready"
                }
                .into(),
            );
            ui.set_footer_center(
                if processing == 1 {
                    "1 file processing".into()
                } else {
                    format!("{processing} files processing").into()
                },
            );

            if let Some(row) = state_for_image {
                ui.set_selected_name(row.name().into());
                ui.set_selected_metadata(
                    format!(
                        "{}×{}   •   {}   •   {}",
                        row.width,
                        row.height,
                        format_duration(row.duration_secs),
                        format_bytes(row.original_bytes)
                    )
                    .into(),
                );
                let encoder = row.encoder.clone().unwrap_or_else(|| "Awaiting encoder selection".to_owned());
                ui.set_selected_encoder(
                    if encoder.contains("videotoolbox") {
                        format!("Encoding with {encoder} (Hardware)")
                    } else {
                        format!("Encoding with {encoder}")
                    }
                    .into(),
                );
                let elapsed = row.started_at.map(|value| value.elapsed().as_secs_f64()).unwrap_or_default();
                let remaining = if row.progress > 0.0 && elapsed > 0.0 {
                    elapsed * (1.0 / row.progress as f64 - 1.0)
                } else {
                    0.0
                };
                ui.set_selected_eta(format_duration(remaining).into());
                ui.set_selected_elapsed(format_duration(elapsed).into());
                ui.set_selected_speed(format!("{:.2}×", row.speed).into());
                let compression = row
                    .output_bytes
                    .map(|value| 100.0 * (1.0 - value as f64 / row.original_bytes.max(1) as f64))
                    .unwrap_or(0.0);
                ui.set_selected_compression(format!("{compression:.0}%").into());
                ui.set_selected_progress(row.progress);
                if let Some(path) = row.preview_path {
                    if let Ok(image) = Image::load_from_path(&path) {
                        ui.set_preview_image(image);
                    }
                }
            }
        }
    });
}

fn scan_videos(root: &Path, recursive: bool) -> Vec<VideoRow> {
    let walker = if recursive {
        WalkDir::new(root)
    } else {
        WalkDir::new(root).max_depth(1)
    };

    walker
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_video(entry.path()))
        .map(|entry| {
            let path = entry.into_path();
            let original_bytes = fs::metadata(&path).map(|metadata| metadata.len()).unwrap_or(0);
            let probe = probe_video(&path).unwrap_or(ProbeInfo {
                width: 0,
                height: 0,
                duration_secs: 0.0,
            });
            VideoRow {
                path,
                state: FileState::Queued,
                progress: 0.0,
                original_bytes,
                output_bytes: None,
                width: probe.width,
                height: probe.height,
                duration_secs: probe.duration_secs,
                preview_path: None,
                encoder: None,
                started_at: None,
                speed: 0.0,
                message: String::new(),
            }
        })
        .collect()
}

fn select_encoder(config: &JobConfig) -> Result<String> {
    let software = match config.codec {
        Codec::H264 => "libx264",
        Codec::H265 => "libx265",
    };
    let hardware = match config.codec {
        Codec::H264 => "h264_videotoolbox",
        Codec::H265 => "hevc_videotoolbox",
    };

    if !config.use_hardware {
        return Ok(software.to_owned());
    }

    match config.encoder_mode {
        EncoderMode::Software => Ok(software.to_owned()),
        EncoderMode::VideoToolbox => {
            anyhow::ensure!(ffmpeg_has_encoder(hardware), "FFmpeg does not provide {hardware}");
            Ok(hardware.to_owned())
        }
        EncoderMode::Auto => {
            if ffmpeg_has_encoder(hardware) {
                Ok(hardware.to_owned())
            } else {
                Ok(software.to_owned())
            }
        }
    }
}

fn ffmpeg_has_encoder(name: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(name))
        .unwrap_or(false)
}

fn create_preview_frame(input: &Path, output: &Path, duration_secs: f64) -> Result<()> {
    let seek = (duration_secs * 0.10).clamp(1.0, 300.0);
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{seek:.3}"), "-i"])
        .arg(input)
        .args(["-frames:v", "1", "-vf", "scale=640:-2", "-q:v", "3"])
        .arg(output)
        .status()?;
    anyhow::ensure!(status.success(), "unable to create preview frame");
    Ok(())
}

fn create_contact_sheet(input: &Path, partial: &Path, final_path: &Path) -> Result<()> {
    let _ = fs::remove_file(partial);
    let filter = "fps=1/60,scale=320:-2,tile=4x3:nb_frames=12:padding=4:margin=4";
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vf", filter, "-frames:v", "1", "-f", "image2"])
        .arg(partial)
        .status()?;
    anyhow::ensure!(status.success(), "FFmpeg failed while creating the contact sheet");
    if final_path.exists() {
        fs::remove_file(final_path)?;
    }
    fs::rename(partial, final_path)?;
    Ok(())
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

#[derive(Debug, Clone)]
struct ProbeInfo {
    width: u32,
    height: u32,
    duration_secs: f64,
}

fn probe_video(path: &Path) -> Result<ProbeInfo> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_streams", "-show_format"])
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

fn is_video(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("mp4" | "mkv" | "mov" | "avi" | "m4v" | "webm" | "wmv" | "mpg" | "mpeg" | "ts" | "mts" | "m2ts")
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

fn format_duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".to_owned();
    }
    let total = seconds.round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn show_message(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("Video Squeezer")
        .set_description(message)
        .set_level(rfd::MessageLevel::Info)
        .show();
}
