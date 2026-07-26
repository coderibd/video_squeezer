# Testing strategy

## Unit tests

Pure logic is tested beside the implementation under `#[cfg(test)]` modules.
Current coverage includes:

- byte and duration formatting
- supported video-extension recognition
- generated filename sanitization
- queue-state labels and icons
- software and VideoToolbox encoder-selection policy

These tests do not require FFmpeg and run quickly.

## Integration tests

A future integration-test fixture should create very small synthetic videos with FFmpeg and verify:

1. FFprobe metadata parsing.
2. FFmpeg command construction.
3. Partial-file promotion after success.
4. Partial-file deletion after cancellation.
5. Contact-sheet output.

Large copyrighted media files must not be committed as test fixtures.

## Manual release test

Before releasing, process a folder containing:

- one compliant 720p file
- one oversized 4K file
- one video with no audio stream
- one filename containing spaces and punctuation

Run at least two concurrent workers, pause and resume once, then cancel a separate run. Verify that originals remain unchanged and no stale partial output remains.
