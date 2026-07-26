# Contributing

## Setup

1. Install Rust stable through `rustup`.
2. Install FFmpeg with `brew install ffmpeg`.
3. Clone the repository.
4. Run `make quality` before changing code.

## Development loop

```bash
make fmt
make check
make test
make run
```

Before opening a pull request:

```bash
make quality
```

## Code organization

Put code in the narrowest appropriate layer:

- GUI callback or Slint property update: `src/app`
- shared data type: `src/models`
- scheduling or worker-pool behavior: `src/scheduler`
- FFmpeg, FFprobe, filesystem scanning, thumbnails: `src/services`
- small pure helper: `src/utils`

Do not launch FFmpeg from GUI callbacks. Do not update Slint widgets from a worker thread.

## Commenting style

Comments should explain intent, safety constraints, and non-obvious decisions. Avoid comments that merely repeat the next line of code.

Public types and functions should have Rustdoc comments (`///`) that explain their contract. Internal implementation notes should use ordinary comments (`//`).

## Commit expectations

- Keep changes focused.
- Add or update tests for pure logic.
- Update `CHANGELOG.md` for user-visible changes.
- Avoid adding dependencies when the standard library is sufficient.
