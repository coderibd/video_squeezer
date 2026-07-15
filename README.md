# video-squeezer

A Rust CLI that recursively scans a drive or directory, probes each video with `ffprobe`, creates a labeled thumbnail contact sheet, and compresses videos that are larger than the configured size or resolution.

## Requirements

- Rust 1.74 or newer
- FFmpeg and FFprobe available in `PATH`
- An FFmpeg build containing the selected encoder (`libx265` by default) and the `drawtext` filter

## Build

```bash
cargo clean
cargo fmt
cargo build --release
cargo doc --open
```

## Example

```bash
./target/release/video-squeezer \
  /media/archive \
  --output /media/processed \
  --target-mib 1000 \
  --max-width 1280 \
  --max-height 720 \
  --codec h265 \
  --preset medium \
  --jobs 1
```

Windows PowerShell:

```powershell
.\target\release\video-squeezer.exe `
  "E:\" `
  --output "D:\processed-videos" `
  --target-mib 1000 `
  --jobs 1
```

Start with `--dry-run` to inspect what will happen without writing files.

## Important behavior

- The output directory mirrors the input directory tree.
- Contact sheets are named `<original-name>.contact-sheet.jpg`.
- Compressed videos are named `<original-name>.compressed.mp4`.
- Existing outputs are preserved unless `--overwrite` is supplied.
- Files are written to temporary `.partial` paths, validated, then renamed.
- Output-size targeting uses bitrate budgeting plus a configurable safety margin. Exact encoded sizes vary slightly by container and encoder.
