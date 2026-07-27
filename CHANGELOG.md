# Changelog

## 2.2.0 - Compression Advisor

- Add codec-aware target bitrate and quality estimates before encoding.
- Make maximum resolution and target size part of one explicit compression plan.
- Never upscale sources that are smaller than the selected maximum resolution.
- Add Balanced, Best Quality, Smallest File, and Fastest Encode strategies.
- Add configurable automatic retries when an encoded file exceeds its target size.
- Display predicted output size, target bitrate, quality rating, frame rate, and advice.
- Show predicted output sizes in the processing queue before encoding starts.
- Add unit tests for resolution fitting, frame-rate parsing, bitrate planning, and retries.

All notable changes are documented here.

## 2.1.0 — 2026-07-26

### Added

- macOS GitHub Actions quality gate
- Makefile development commands
- local format, lint, test, and documentation scripts
- architecture, contribution, maintenance, security, and roadmap documentation
- unit tests for formatting, path handling, queue states, and encoder selection

### Changed

- package metadata now identifies the project as an MIT-licensed desktop application
- source formatting normalized to the standard Rust formatter
- encoder selection split into a pure decision function and an FFmpeg capability check

## 2.0.1

- removed an unused service re-export
- made FFmpeg encoder detection an internal service detail

## 2.0.0

- reorganized the application into app, models, scheduler, services, and utilities layers
