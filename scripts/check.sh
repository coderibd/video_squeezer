#!/usr/bin/env bash
set -euo pipefail

step() {
  printf '\n==> %s\n' "$1"
}

step "Checking formatting"
cargo fmt --all -- --check

step "Type-checking every target"
cargo check --all-targets

step "Running Clippy with warnings denied"
cargo clippy --all-targets --all-features -- -D warnings

step "Running tests"
cargo test --all-targets

step "Building developer documentation"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

printf '\nAll quality checks passed.\n'
