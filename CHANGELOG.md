# Changelog

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
