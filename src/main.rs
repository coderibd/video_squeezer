//! Video Squeezer
//!
//! Recursively scans a directory tree for video files, probes them with
//! `ffprobe`, compresses files that exceed the configured size or resolution,
//! and creates a thumbnail contact sheet for every discovered video.
//!
//! On macOS, the default `auto` encoder mode prefers Apple VideoToolbox for
//! substantially faster hardware-assisted H.264 or HEVC encoding. The program
//! falls back to software encoding when the requested hardware encoder is not
//! available. Originals are never modified or deleted.

// Error handling, command-line parsing, parallel iteration, JSON decoding,
// filesystem traversal, and child-process execution dependencies.
use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

// Binary mebibyte used consistently for input and output size comparisons.
const MIB: u64 = 1024 * 1024;

/// Video compression format requested by the user.
///
/// The selected codec is mapped separately to software and VideoToolbox
/// encoder names because FFmpeg exposes them as different encoders.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum VideoCodec {
    H264,
    H265,
    Av1,
}

impl VideoCodec {
    /// Return the FFmpeg CPU encoder corresponding to this codec.
    fn software_encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H265 => "libx265",
            Self::Av1 => "libsvtav1",
        }
    }

    /// Return the Apple VideoToolbox encoder when this codec is supported.
    /// AV1 intentionally returns `None` because this program has no
    /// VideoToolbox AV1 path.
    fn hardware_encoder(self) -> Option<&'static str> {
        match self {
            Self::H264 => Some("h264_videotoolbox"),
            Self::H265 => Some("hevc_videotoolbox"),
            Self::Av1 => None,
        }
    }

    /// Choose a speed-oriented default preset for CPU encoding.
    /// Users can override this value with `--preset`.
    fn default_software_preset(self) -> &'static str {
        match self {
            Self::H264 | Self::H265 => "veryfast",
            Self::Av1 => "8",
        }
    }
}

/// Selects whether encoding is performed by the CPU or Apple hardware.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum EncoderMode {
    /// Use VideoToolbox on macOS when available, otherwise use software encoding.
    Auto,
    /// Always use CPU-based encoding.
    Software,
    /// Require Apple's VideoToolbox hardware encoder.
    Videotoolbox,
}

/// Complete command-line configuration parsed by Clap.
///
/// Defaults favor safe unattended processing: originals remain untouched,
/// output is written under a separate root, and one large file is encoded at
/// a time unless the user explicitly increases `--jobs`.
#[derive(Debug, Parser)]
#[command(
    name = "video-squeezer",
    version,
    about = "Recursively compress oversized/high-resolution videos and generate contact sheets"
)]
struct Args {
    /// Drive or directory to scan recursively.
    #[arg(value_name = "INPUT_ROOT")]
    input_root: PathBuf,

    /// Root directory for compressed videos and contact sheets.
    #[arg(short, long, value_name = "DIR")]
    output: PathBuf,

    /// Maximum output width.
    #[arg(long, default_value_t = 1280)]
    max_width: u32,

    /// Maximum output height.
    #[arg(long, default_value_t = 720)]
    max_height: u32,

    /// Target maximum output size in MiB. A safety margin is applied.
    #[arg(long, default_value_t = 1000)]
    target_mib: u64,

    /// Fraction of target size reserved as a safety margin (0.03 = 3%).
    #[arg(long, default_value_t = 0.03, value_parser = parse_size_margin)]
    size_margin: f64,

    /// Video codec used for transcoding.
    #[arg(long, value_enum, default_value_t = VideoCodec::H265)]
    codec: VideoCodec,

    /// Encoder implementation. Auto uses VideoToolbox on supported Macs.
    #[arg(long, value_enum, default_value_t = EncoderMode::Auto)]
    encoder: EncoderMode,

    /// Software encoder speed/preset. Ignored by VideoToolbox.
    #[arg(long)]
    preset: Option<String>,

    /// Number of files processed concurrently. One is recommended for large videos.
    #[arg(short = 'j', long, default_value_t = 1, value_parser = parse_positive_usize)]
    jobs: usize,

    /// Number of thumbnails in each contact sheet.
    #[arg(long, default_value_t = 12, value_parser = parse_thumbnail_count)]
    thumbnails: u32,

    /// Number of columns in each contact sheet.
    #[arg(long, default_value_t = 4, value_parser = parse_thumbnail_columns)]
    thumbnail_columns: u32,

    /// Width of each thumbnail in pixels.
    #[arg(long, default_value_t = 320)]
    thumbnail_width: u32,

    /// Replace existing generated files.
    #[arg(long)]
    overwrite: bool,

    /// Probe and report actions without writing output files.
    #[arg(long)]
    dry_run: bool,

    /// Include symbolic links while walking the input tree.
    #[arg(long)]
    follow_links: bool,

    /// Keep audio bitrate at or below this many kbit/s.
    #[arg(long, default_value_t = 128)]
    audio_kbps: u32,

    /// Paths containing any of these directory names are skipped.
    #[arg(
        long,
        value_delimiter = ',',
        default_value = ".git,@eaDir,$RECYCLE.BIN,System Volume Information"
    )]
    exclude_dirs: Vec<String>,
}

/// Top-level portion of the JSON document returned by `ffprobe`.
#[derive(Debug, Deserialize)]
struct ProbeOutput {
    streams: Vec<ProbeStream>,
    format: ProbeFormat,
}

/// Stream fields needed from each `ffprobe` stream record.
#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<String>,
}

/// Container-level duration and byte-size fields reported by `ffprobe`.
#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    size: Option<String>,
}

/// Normalized metadata used by the processing and bitrate calculations.
#[derive(Debug)]
struct VideoInfo {
    width: u32,
    height: u32,
    duration_secs: f64,
    size_bytes: u64,
}

/// Resolved FFmpeg encoder and whether it is hardware accelerated.
#[derive(Debug, Clone)]
struct SelectedEncoder {
    name: &'static str,
    hardware: bool,
}

/// Per-file counters merged into the final process summary.
#[derive(Default)]
struct Outcome {
    compressed: usize,
    contact_sheet: usize,
    skipped: usize,
}

/// Program entry point: validate configuration, select an encoder, discover
/// files, process them in parallel, and report aggregate results.
fn main() -> Result<()> {
    let args = Args::parse();
    validate_environment(&args)?;
    let selected_encoder = select_encoder(&args)?;

    println!(
        "Encoder: {} ({})",
        selected_encoder.name,
        if selected_encoder.hardware {
            "Apple VideoToolbox hardware"
        } else {
            "software"
        }
    );

    // Configure Rayon once so `--jobs` limits the number of videos that may be
    // encoded simultaneously. This does not change FFmpeg's internal threads.
    rayon::ThreadPoolBuilder::new()
        .num_threads(args.jobs)
        .build_global()
        .context("failed to configure worker pool")?;

    let videos = discover_videos(&args)?;
    println!("Found {} candidate video file(s)", videos.len());

    let compressed = AtomicUsize::new(0);
    let sheets = AtomicUsize::new(0);
    let skipped = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);

    // Each file is independent. Atomic counters avoid locking while worker
    // threads update the final summary.
    videos
        .par_iter()
        .for_each(|path| match process_video(path, &args, &selected_encoder) {
            Ok(outcome) => {
                compressed.fetch_add(outcome.compressed, Ordering::Relaxed);
                sheets.fetch_add(outcome.contact_sheet, Ordering::Relaxed);
                skipped.fetch_add(outcome.skipped, Ordering::Relaxed);
            }
            Err(error) => {
                failed.fetch_add(1, Ordering::Relaxed);
                eprintln!("ERROR: {}: {error:#}", path.display());
            }
        });

    println!(
        "Done: compressed={}, contact_sheets={}, skipped={}, failed={}",
        compressed.load(Ordering::Relaxed),
        sheets.load(Ordering::Relaxed),
        skipped.load(Ordering::Relaxed),
        failed.load(Ordering::Relaxed)
    );

    if failed.load(Ordering::Relaxed) > 0 {
        bail!("one or more files failed");
    }
    Ok(())
}

/// Parse and constrain the safety margin used for output-size targeting.
fn parse_size_margin(value: &str) -> Result<f64, String> {
    let margin = value
        .parse::<f64>()
        .map_err(|_| format!("invalid size margin: {value}"))?;
    if !margin.is_finite() || !(0.0..0.25).contains(&margin) {
        return Err("size margin must be at least 0.0 and less than 0.25".to_string());
    }
    Ok(margin)
}

/// Parse a non-zero worker count for Rayon.
fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid positive integer: {value}"))?;
    if parsed == 0 {
        return Err("value must be at least 1".to_string());
    }
    Ok(parsed)
}

/// Parse the requested number of contact-sheet frames.
fn parse_thumbnail_count(value: &str) -> Result<u32, String> {
    parse_u32_range(value, 1, 100, "thumbnail count")
}

/// Parse the number of columns used by FFmpeg's tile filter.
fn parse_thumbnail_columns(value: &str) -> Result<u32, String> {
    parse_u32_range(value, 1, 20, "thumbnail columns")
}

/// Shared bounded integer parser used by thumbnail-related options.
fn parse_u32_range(value: &str, min: u32, max: u32, label: &str) -> Result<u32, String> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| format!("invalid {label}: {value}"))?;
    if !(min..=max).contains(&parsed) {
        return Err(format!("{label} must be between {min} and {max}"));
    }
    Ok(parsed)
}

/// Validate arguments, ensure FFmpeg tools are callable, and create the output
/// directory before any parallel work begins.
fn validate_environment(args: &Args) -> Result<()> {
    if !args.input_root.is_dir() {
        bail!(
            "input root is not a directory: {}",
            args.input_root.display()
        );
    }
    if args.target_mib < 16 {
        bail!("--target-mib must be at least 16");
    }
    if args.max_width < 2 || args.max_height < 2 {
        bail!("--max-width and --max-height must be at least 2");
    }
    if args.thumbnail_columns > args.thumbnails {
        bail!("--thumbnail-columns cannot exceed --thumbnails");
    }
    check_program("ffmpeg")?;
    check_program("ffprobe")?;
    if !args.dry_run {
        fs::create_dir_all(&args.output)
            .with_context(|| format!("cannot create output directory {}", args.output.display()))?;
    }
    Ok(())
}

/// Confirm an external executable exists in `PATH` and can run successfully.
fn check_program(name: &str) -> Result<()> {
    let status = Command::new(name)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("{name} is not installed or not in PATH"))?;
    if !status.success() {
        bail!("{name} exists but failed its version check");
    }
    Ok(())
}

/// Resolve the requested codec and encoder mode to a concrete FFmpeg encoder.
///
/// In automatic mode on macOS, hardware encoding is preferred when available.
/// All other cases resolve to the corresponding software encoder.
fn select_encoder(args: &Args) -> Result<SelectedEncoder> {
    let software = SelectedEncoder {
        name: args.codec.software_encoder(),
        hardware: false,
    };

    match args.encoder {
        EncoderMode::Software => {
            ensure_encoder_available(software.name)?;
            Ok(software)
        }
        EncoderMode::Videotoolbox => {
            let name = args.codec.hardware_encoder().context(
                "VideoToolbox does not support AV1 in this program; use --encoder software",
            )?;
            ensure_encoder_available(name)?;
            Ok(SelectedEncoder {
                name,
                hardware: true,
            })
        }
        EncoderMode::Auto => {
            if cfg!(target_os = "macos") {
                if let Some(name) = args.codec.hardware_encoder() {
                    if encoder_available(name)? {
                        return Ok(SelectedEncoder {
                            name,
                            hardware: true,
                        });
                    }
                    eprintln!(
                        "WARN: {name} is unavailable in this FFmpeg build; falling back to {}",
                        software.name
                    );
                }
            }
            ensure_encoder_available(software.name)?;
            Ok(software)
        }
    }
}

/// Convert an unavailable FFmpeg encoder into a user-facing error.
fn ensure_encoder_available(name: &str) -> Result<()> {
    if !encoder_available(name)? {
        bail!("FFmpeg encoder '{name}' is not available in your installation");
    }
    Ok(())
}

/// Inspect `ffmpeg -encoders` and test for an exact encoder-name match.
fn encoder_available(name: &str) -> Result<bool> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .context("failed to query FFmpeg encoders")?;
    if !output.status.success() {
        bail!("FFmpeg failed while listing encoders");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.split_whitespace().any(|field| field == name)))
}

/// Walk the input tree, skip excluded/output directories, and return supported
/// video paths in deterministic sorted order.
fn discover_videos(args: &Args) -> Result<Vec<PathBuf>> {
    let output_abs = absolute_normalized(&args.output);
    let mut result = Vec::new();

    let walker = WalkDir::new(&args.input_root)
        .follow_links(args.follow_links)
        .into_iter()
        // Pruning directories here prevents WalkDir from descending into them,
        // which is faster than filtering their files after traversal.
        .filter_entry(|entry| {
            let path = absolute_normalized(entry.path());
            if path.starts_with(&output_abs) {
                return false;
            }
            if entry.file_type().is_dir() {
                let name = entry.file_name().to_string_lossy();
                return !args
                    .exclude_dirs
                    .iter()
                    .any(|excluded| name.eq_ignore_ascii_case(excluded));
            }
            true
        });

    for entry in walker {
        match entry {
            Ok(entry) if entry.file_type().is_file() && is_video(entry.path()) => {
                result.push(entry.into_path());
            }
            Ok(_) => {}
            Err(error) => eprintln!("WARN: traversal error: {error}"),
        }
    }
    result.sort();
    Ok(result)
}

/// Determine whether a path has one of the supported video extensions.
fn is_video(path: &Path) -> bool {
    const EXTENSIONS: &[&str] = &[
        "3gp", "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "ogv", "ts",
        "vob", "webm", "wmv",
    ];
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// Process one video from probe through optional compression and contact-sheet
/// generation. Output paths mirror the source directory structure.
fn process_video(input: &Path, args: &Args, encoder: &SelectedEncoder) -> Result<Outcome> {
    let info = probe_video(input)?;
    let relative = input.strip_prefix(&args.input_root).unwrap_or(input);
    let relative_parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let output_dir = args.output.join(relative_parent);
    let stem = input
        .file_stem()
        .and_then(OsStr::to_str)
        .context("video filename is not valid Unicode")?;

    let output_video = output_dir.join(format!("{stem}.compressed.mp4"));
    let contact_sheet = output_dir.join(format!("{stem}.contact-sheet.jpg"));

    // A file is transcoded when either threshold is exceeded. A video already
    // within both limits is left untouched, but still receives a contact sheet.
    let too_large = info.size_bytes > args.target_mib * MIB;
    let too_high_res = info.width > args.max_width || info.height > args.max_height;
    let should_compress = too_large || too_high_res;

    println!(
        "{}: {}x{}, {:.2} MiB, {:.1}s -> {}",
        input.display(),
        info.width,
        info.height,
        info.size_bytes as f64 / MIB as f64,
        info.duration_secs,
        if should_compress {
            format!("compress with {}", encoder.name)
        } else {
            "thumbnail only".to_string()
        }
    );

    if args.dry_run {
        return Ok(Outcome {
            compressed: usize::from(should_compress),
            contact_sheet: 1,
            skipped: usize::from(!should_compress),
        });
    }

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("cannot create {}", output_dir.display()))?;

    let mut outcome = Outcome::default();

    if should_compress {
        compress_to_target(input, &output_video, &info, args, encoder)?;
        outcome.compressed = 1;
    } else {
        outcome.skipped = 1;
    }

    // Generate the relatively inexpensive contact sheet after a successful encode.
    generate_contact_sheet(input, &contact_sheet, &info, args)?;
    outcome.contact_sheet = 1;

    Ok(outcome)
}

/// Run `ffprobe`, decode its JSON response, and normalize required metadata.
fn probe_video(path: &Path) -> Result<VideoInfo> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("failed to run ffprobe for {}", path.display()))?;

    if !output.status.success() {
        bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let parsed: ProbeOutput = serde_json::from_slice(&output.stdout)
        .with_context(|| format!("invalid ffprobe JSON for {}", path.display()))?;
    let video = parsed
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"))
        .context("no video stream found")?;

    let duration_secs = parsed
        .format
        .duration
        .as_deref()
        .or(video.duration.as_deref())
        .context("duration is unavailable")?
        .parse::<f64>()
        .context("invalid duration")?;
    if !duration_secs.is_finite() || duration_secs <= 0.0 {
        bail!("invalid duration: {duration_secs}");
    }

    // Some containers omit the size field. Filesystem metadata provides a
    // reliable fallback without requiring another media-tool invocation.
    let size_bytes = match parsed
        .format
        .size
        .as_deref()
        .and_then(|size| size.parse().ok())
    {
        Some(size) => size,
        None => fs::metadata(path)?.len(),
    };

    Ok(VideoInfo {
        width: video.width.context("video width is unavailable")?,
        height: video.height.context("video height is unavailable")?,
        duration_secs,
        size_bytes,
    })
}

/// Generate an evenly sampled JPEG thumbnail collage using FFmpeg filters.
///
/// No `drawtext` filter is used, keeping this compatible with minimal FFmpeg
/// builds. The output filename associates the sheet with its source video.
fn generate_contact_sheet(
    input: &Path,
    output: &Path,
    info: &VideoInfo,
    args: &Args,
) -> Result<()> {
    if output.exists() && !args.overwrite {
        println!("  contact sheet exists; skipping: {}", output.display());
        return Ok(());
    }

    // Sample frames at regular intervals while avoiding the exact beginning
    // and end, where fades or blank frames are more common.
    let rows = args.thumbnails.div_ceil(args.thumbnail_columns);
    let interval = (info.duration_secs / (args.thumbnails as f64 + 1.0)).max(0.1);
    let filter = format!(
        "fps=1/{interval:.6},scale={}:-1,tile={}x{}:nb_frames={}:padding=6:margin=6",
        args.thumbnail_width, args.thumbnail_columns, rows, args.thumbnails
    );

    // FFmpeg writes to a hidden partial file. Only a completed image is renamed
    // into its final path, so interrupted runs do not leave valid-looking files.
    let temp = temporary_path(output);
    remove_stale_temp(&temp)?;

    // Build a single-frame FFmpeg filter pipeline that samples, scales, and
    // tiles the requested number of thumbnails into one JPEG image.
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-stats_period",
            "0.5",
            "-progress",
            "pipe:1",
        ])
        .arg("-y")
        .arg("-ss")
        .arg(format!("{interval:.3}"))
        .arg("-i")
        .arg(input)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(filter)
        .arg("-q:v")
        .arg("2")
        .arg(&temp);

    run_ffmpeg_with_progress(command, "Generating thumbnail collage", info.duration_secs)?;
    atomic_replace(&temp, output, args.overwrite)?;
    Ok(())
}

/// Transcode a video using a duration-derived bitrate budget.
///
/// The total target bytes are converted into bits per second, with a bounded
/// share reserved for AAC audio. The remaining bitrate is assigned to video.
fn compress_to_target(
    input: &Path,
    output: &Path,
    info: &VideoInfo,
    args: &Args,
    encoder: &SelectedEncoder,
) -> Result<()> {
    if output.exists() && !args.overwrite {
        println!("  compressed output exists; skipping: {}", output.display());
        return Ok(());
    }

    // Reserve the configured margin because container overhead and encoder
    // rate-control variation can otherwise push the finished file over target.
    let target_bytes = (args.target_mib * MIB) as f64 * (1.0 - args.size_margin);
    let total_bps = target_bytes * 8.0 / info.duration_secs;
    let audio_bps = (args.audio_kbps as f64 * 1000.0).min(total_bps * 0.20);
    let video_bps = total_bps - audio_bps;
    if video_bps < 100_000.0 {
        bail!(
            "target size is unrealistic for {:.1}s duration (video bitrate would be {:.0} bps)",
            info.duration_secs,
            video_bps
        );
    }

    let maxrate = (video_bps * 1.10).round() as u64;
    let bufsize = (video_bps * 2.0).round() as u64;
    let bitrate = video_bps.round() as u64;
    // Downscale only when needed, preserve aspect ratio, and force even
    // dimensions because common H.264/HEVC pixel formats require them.
    let scale = format!(
        "scale=w='min(iw,{})':h='min(ih,{})':force_original_aspect_ratio=decrease:force_divisible_by=2",
        args.max_width, args.max_height
    );
    let temp = temporary_path(output);
    remove_stale_temp(&temp)?;

    println!(
        "  target video bitrate: {:.0} kbit/s, audio: {:.0} kbit/s",
        video_bps / 1000.0,
        audio_bps / 1000.0
    );

    // Build the compression command incrementally because VideoToolbox and
    // software encoders accept different rate-control and preset options.
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostats",
            "-stats_period",
            "0.5",
            "-progress",
            "pipe:1",
        ])
        .arg("-y")
        .arg("-i")
        .arg(input)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("0:a?")
        .arg("-map_metadata")
        .arg("0")
        .arg("-map_chapters")
        .arg("0")
        .arg("-vf")
        .arg(scale)
        .arg("-c:v")
        .arg(encoder.name);

    if encoder.hardware {
        // VideoToolbox does not accept libx264/libx265 preset names.
        command.arg("-b:v").arg(bitrate.to_string());
        if matches!(args.codec, VideoCodec::H265) {
            command.arg("-tag:v").arg("hvc1");
        }
    } else {
        let preset = args
            .preset
            .as_deref()
            .unwrap_or(args.codec.default_software_preset());
        command
            .arg("-preset")
            .arg(preset)
            .arg("-b:v")
            .arg(bitrate.to_string())
            .arg("-maxrate")
            .arg(maxrate.to_string())
            .arg("-bufsize")
            .arg(bufsize.to_string());
    }

    command
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", (audio_bps / 1000.0).round() as u64))
        .arg("-movflags")
        .arg("+faststart")
        .arg(&temp);

    run_ffmpeg_with_progress(command, "Encoding video", info.duration_secs)?;

    // Enforce the user's hard size ceiling before publishing the partial file.
    // A failed size check deletes the temporary output and leaves any existing
    // completed output untouched.
    let actual = fs::metadata(&temp)?.len();
    if actual > args.target_mib * MIB {
        let _ = fs::remove_file(&temp);
        bail!(
            "encoded output is {:.2} MiB, above target {} MiB; increase --size-margin (for example, --size-margin 0.06)",
            actual as f64 / MIB as f64,
            args.target_mib
        );
    }

    atomic_replace(&temp, output, args.overwrite)?;
    Ok(())
}

/// Execute FFmpeg while presenting a live, single-line progress display.
///
/// FFmpeg's `-progress pipe:1` option emits stable `key=value` records. We
/// parse the current media timestamp and reported speed instead of scraping
/// human-oriented log output. The media timestamp divided by the known source
/// duration provides completion percentage, while elapsed wall-clock time is
/// used to estimate the remaining duration.
fn run_ffmpeg_with_progress(
    mut command: Command,
    operation: &str,
    duration_secs: f64,
) -> Result<()> {
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to start ffmpeg for {operation}"))?;
    let stdout = child
        .stdout
        .take()
        .context("failed to capture FFmpeg progress output")?;

    // Read FFmpeg output on a helper thread so the main thread can refresh the
    // display even during periods when FFmpeg emits no new progress record.
    let (sender, receiver) = mpsc::channel::<String>();
    let reader_thread = thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let started = Instant::now();
    let mut processed_secs = 0.0_f64;
    let mut reported_speed = String::from("--");
    let mut finished_progress_stream = false;

    eprint!("  {operation}: starting...");
    let _ = std::io::stderr().flush();

    while !finished_progress_stream {
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(line) => {
                if let Some((key, value)) = line.split_once('=') {
                    match key {
                        // Modern FFmpeg reports microseconds in `out_time_us`.
                        "out_time_us" => {
                            if let Ok(microseconds) = value.parse::<f64>() {
                                processed_secs = microseconds / 1_000_000.0;
                            }
                        }
                        // Retained for compatibility with builds that expose
                        // only `out_time_ms`; despite the name, FFmpeg has
                        // historically reported this field in microseconds.
                        "out_time_ms" if processed_secs == 0.0 => {
                            if let Ok(microseconds) = value.parse::<f64>() {
                                processed_secs = microseconds / 1_000_000.0;
                            }
                        }
                        "speed" => reported_speed = value.trim().to_string(),
                        "progress" if value == "end" => {
                            finished_progress_stream = true;
                            processed_secs = duration_secs;
                        }
                        _ => {}
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                finished_progress_stream = true;
            }
        }

        render_progress_line(
            operation,
            processed_secs,
            duration_secs,
            started.elapsed(),
            &reported_speed,
        );
    }

    let status = child
        .wait()
        .with_context(|| format!("failed while waiting for ffmpeg during {operation}"))?;
    let _ = reader_thread.join();

    if !status.success() {
        eprintln!();
        bail!("ffmpeg failed during {operation} with status {status}");
    }

    // Finish the line at exactly 100%, then move subsequent messages onto a
    // fresh terminal line.
    render_progress_line(
        operation,
        duration_secs,
        duration_secs,
        started.elapsed(),
        &reported_speed,
    );
    eprintln!();
    Ok(())
}

/// Render a compact terminal progress bar with timing and speed information.
fn render_progress_line(
    operation: &str,
    processed_secs: f64,
    duration_secs: f64,
    elapsed: Duration,
    speed: &str,
) {
    let fraction = if duration_secs > 0.0 {
        (processed_secs / duration_secs).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let percent = fraction * 100.0;
    let bar_width = 30_usize;
    let filled = (fraction * bar_width as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "=".repeat(filled.min(bar_width)),
        " ".repeat(bar_width.saturating_sub(filled))
    );

    let eta = if fraction > 0.001 && fraction < 1.0 {
        let remaining = elapsed.as_secs_f64() * (1.0 - fraction) / fraction;
        format_duration(Duration::from_secs_f64(remaining.max(0.0)))
    } else if fraction >= 1.0 {
        "00:00".to_string()
    } else {
        "--:--".to_string()
    };

    let spinner = ["|", "/", "-", "\\"][(elapsed.as_millis() / 250) as usize % 4];
    let activity = if processed_secs <= 0.0 { spinner } else { " " };

    eprint!(
        "\r  {} {:<27} [{}] {:6.2}%  elapsed {}  ETA {}  speed {:>7}",
        activity,
        operation,
        bar,
        percent,
        format_duration(elapsed),
        eta,
        speed
    );
    let _ = std::io::stderr().flush();
}

/// Format a duration as `HH:MM:SS` for long jobs or `MM:SS` for short ones.
fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3_600;
    let minutes = (total % 3_600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Construct a hidden sibling path that preserves the final extension so
/// FFmpeg can infer the desired output container or image format.
fn temporary_path(final_path: &Path) -> PathBuf {
    let stem = final_path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("output");
    let extension = final_path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("tmp");
    final_path.with_file_name(format!(".{stem}.partial.{extension}"))
}

/// Remove a partial file left by an interrupted or failed earlier run.
fn remove_stale_temp(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("cannot remove stale temporary file {}", path.display()))?;
    }
    Ok(())
}

/// Publish a completed temporary file under its final name.
///
/// The rename occurs only after FFmpeg succeeds and all validation completes.
fn atomic_replace(temp: &Path, final_path: &Path, overwrite: bool) -> Result<()> {
    if final_path.exists() {
        if overwrite {
            fs::remove_file(final_path)
                .with_context(|| format!("cannot replace {}", final_path.display()))?;
        } else {
            let _ = fs::remove_file(temp);
            bail!("output already exists: {}", final_path.display());
        }
    }
    fs::rename(temp, final_path).with_context(|| {
        format!(
            "cannot move completed file {} to {}",
            temp.display(),
            final_path.display()
        )
    })
}

/// Convert a relative path to an absolute path without requiring it to exist.
/// This is sufficient for excluding the output tree during directory walking.
fn absolute_normalized(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}
