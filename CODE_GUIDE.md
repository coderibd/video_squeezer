# Video Squeezer 2.1 Code Guide

This guide is written for readers who do not work with Rust every day.

## The application in one sentence

The Slint window gathers settings, the scanner builds a list of videos, the
scheduler gives those videos to several worker threads, and each worker runs
FFmpeg while reporting progress back to the window.

## Source map

```text
src/
├── main.rs                 Starts the application
├── app/
│   ├── mod.rs              Window lifecycle and loading spinner
│   ├── callbacks.rs        Browse, Scan, Start, Pause, Stop, and Help actions
│   ├── settings.rs         Converts GUI controls into validated settings
│   └── view.rs             Copies Rust state into Slint properties
├── models/
│   ├── mod.rs              Re-exports shared model types
│   ├── settings.rs         Codec, encoder mode, and JobConfig
│   ├── state.rs            Thread-safe shared application state
│   └── video.rs            Queue row and file lifecycle
├── scheduler/
│   ├── mod.rs              Scheduler exports
│   ├── pool.rs             Starts and joins concurrent workers
│   └── worker.rs           Processes one video with FFmpeg
├── services/
│   ├── mod.rs              Media-service exports
│   ├── encoder.rs          Selects VideoToolbox or software encoders
│   ├── ffprobe.rs          Reads duration and resolution
│   ├── scanner.rs          Walks folders and builds the queue
│   └── thumbnails.rs       Creates previews and contact sheets
└── utils/
    ├── mod.rs              Utility exports
    ├── dialogs.rs          Native information dialogs
    ├── formatting.rs       Human-readable sizes and times
    └── paths.rs            Extension checks and safe temporary names
```

## Important Rust ideas used here

### `Arc<SharedState>`

`Arc` means "atomically reference-counted." It lets the GUI, scanner, and
worker threads all own a safe reference to the same application state.

### `Mutex<Vec<VideoRow>>`

A mutex permits only one thread at a time to modify the queue. The code keeps
that lock for short operations and clones rows before updating the GUI.

### Atomic flags

`running`, `paused`, `cancel`, and `scanning` are simple true/false flags.
Workers can check these frequently without locking the entire queue.

### `Result`

Functions that can fail return `Result`. The `?` operator stops the current
function and forwards the error to its caller. This avoids silently ignoring
filesystem or FFmpeg failures.

## Processing flow

1. The user selects input and output folders.
2. `app/callbacks.rs` starts a scanner thread.
3. `services/scanner.rs` finds videos and calls FFprobe.
4. The Start button creates a `JobConfig` snapshot.
5. `scheduler/pool.rs` launches the requested number of workers.
6. Each worker claims the next queue index using an atomic counter.
7. `scheduler/worker.rs` creates a preview, decides whether compression is
   needed, starts FFmpeg, and parses its progress output.
8. `app/view.rs` schedules safe updates on the Slint event loop.
9. Successful encodes are renamed from hidden partial files to final outputs.

## Safety decisions

- Source videos are never overwritten or deleted.
- Encodes are written to `.partial.mp4` files first.
- A partial file becomes final only after FFmpeg exits successfully.
- Cancellation kills FFmpeg and removes its partial output.
- A preview-generation failure does not fail the video encode.
- Worker errors affect only the relevant queue row.

## Generating API documentation

Run:

```bash
cargo doc --no-deps --open
```

Rust will build browsable documentation from the `//!` and `///` comments in
these modules.


## Development commands

The repository includes a small Makefile so contributors do not need to
remember every Cargo flag:

```bash
make fmt      # apply standard Rust formatting
make quality  # formatting, check, Clippy, tests, and documentation
make fix      # apply formatting and safe Clippy suggestions
make release  # optimized production build
```

The GitHub Actions workflow runs `scripts/check.sh` on macOS for every push
and pull request. The local and hosted quality gates therefore use the same
commands.
