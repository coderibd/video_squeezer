# Video Squeezer

Video Squeezer is a macOS-first Rust desktop application that scans a folder of videos, identifies files that exceed configured size or resolution limits, compresses them with FFmpeg, and generates preview/contact-sheet images.

The application supports concurrent processing and can use Apple VideoToolbox hardware encoders when available.

## Requirements

- macOS
- Rust stable, installed through `rustup`
- FFmpeg and FFprobe available in `PATH`

Install FFmpeg with Homebrew:

```bash
brew install ffmpeg
```

## Build and run

```bash
cargo build --release
./target/release/video-squeezer
```

During development:

```bash
make run
```

## Quality checks

Run the same checks used by continuous integration:

```bash
make quality
```

Apply standard formatting and safe Clippy fixes:

```bash
make fix
```

## Project map

- `src/app`: Slint window lifecycle, callbacks, settings extraction, and view projection
- `src/models`: shared application data and queue states
- `src/scheduler`: concurrent worker pool and one-file processing lifecycle
- `src/services`: FFmpeg, FFprobe, scanning, previews, and contact sheets
- `src/utils`: small formatting, path, and dialog helpers
- `ui/app.slint`: declarative user interface
- `docs`: architecture and workflow documentation

Start with [CODE_GUIDE.md](CODE_GUIDE.md) if you are new to Rust, then read [ARCHITECTURE.md](ARCHITECTURE.md).

## Safety model

Video Squeezer never modifies source videos. Encoded output is first written to a hidden partial file. The partial file is renamed to its final filename only after FFmpeg exits successfully. Interrupted and failed encodes therefore do not replace valid output files.

## License

MIT. See [LICENSE](LICENSE).


## Compression Advisor

Video Squeezer treats **Target Size** and **Maximum Resolution** as two parts of
one encoding plan:

1. The source is fitted inside the selected maximum resolution without ever
   being enlarged.
2. The target size, duration, audio bitrate, and safety margin are converted
   into an average video bitrate.
3. That bitrate is compared with a codec-, resolution-, and frame-rate-aware
   quality floor.
4. The selected strategy decides whether size, quality, or speed has priority.

The queue can show an estimated output size before processing. Select a file to
see its predicted bitrate, quality rating, and advice. Use **Refresh Compression
Estimates** after changing the target size, resolution, codec, or strategy.

When **Retry when output exceeds target size** is enabled, an oversized encode
is measured and retried at a corrected bitrate. Best Quality will not retry
below its calculated quality floor, so that strategy may intentionally produce
a file larger than the requested target.
