# Video Squeezer — polished Slint GUI

This is a macOS-oriented Rust desktop application that keeps the FFmpeg/FFprobe processing engine and replaces the previous egui front end with a polished Slint interface.

## Features

- macOS-style two-column layout
- colored processing summary cards
- queue table with icons, statuses, progress, original/output sizes, and resolution
- real preview frame for the selected video
- elapsed time, ETA, encoding speed, and compression metrics
- native folder pickers
- H.264 and H.265
- automatic Apple VideoToolbox selection
- software fallback
- pause, stop, parallel jobs, overwrite, skip-compliant, and contact-sheet options
- recursive or top-level-only scanning

## Prerequisites

```bash
brew install ffmpeg
```

Verify:

```bash
ffmpeg -version
ffprobe -version
```

## Replace your existing project

Back up your current folder first, then copy this project's `Cargo.toml`, `build.rs`, `src/`, and `ui/` into it.

## Build

```bash
cargo clean
rm -f Cargo.lock
cargo build --release
```

## Run

```bash
./target/release/video-squeezer
```

The interface is compiled from `ui/app.slint` at build time by `slint-build`, following Slint's recommended Rust integration.
