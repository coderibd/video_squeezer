#!/usr/bin/env bash
set -euo pipefail

printf 'Formatting source files...\n'
cargo fmt --all

printf 'Applying safe Clippy suggestions...\n'
cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged

printf 'Re-running the quality gate...\n'
./scripts/check.sh
