.PHONY: help fmt fmt-check check clippy test doc fix quality release clean run

help:
	@printf '%s\n' \
	  'Video Squeezer development commands' \
	  '' \
	  '  make fmt        Format Rust source' \
	  '  make fmt-check  Verify formatting without changing files' \
	  '  make check      Type-check all targets' \
	  '  make clippy     Run Clippy and reject warnings' \
	  '  make test       Run the test suite' \
	  '  make doc        Build documentation' \
	  '  make fix        Apply safe formatting and Clippy fixes' \
	  '  make quality    Run the complete local quality gate' \
	  '  make release    Build an optimized release binary' \
	  '  make run        Start the application in debug mode'

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

check:
	cargo check --all-targets

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-targets

doc:
	RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --document-private-items

fix:
	cargo fmt --all
	cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged

quality:
	./scripts/check.sh

release:
	cargo build --release

clean:
	cargo clean

run:
	cargo run
