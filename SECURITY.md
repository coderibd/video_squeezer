# Security

Video Squeezer invokes locally installed `ffmpeg` and `ffprobe` executables and processes user-selected media files.

## Reporting a vulnerability

Do not publish exploit details in a public issue. Contact the project maintainer privately with:

- affected version
- operating system and FFmpeg version
- reproduction steps
- expected and observed behavior
- any relevant sample command output with personal paths removed

## Security boundaries

- Source videos are opened read-only by application policy.
- Output is written under the user-selected output directory.
- User-provided paths are passed as separate process arguments, not interpolated into a shell command.
- Temporary files are promoted only after successful processing.
