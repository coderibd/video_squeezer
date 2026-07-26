# Maintenance

## Routine validation

Use the complete local quality gate:

```bash
make quality
```

This checks formatting, type-checks all targets, runs Clippy with warnings denied, runs tests, and builds developer documentation with warnings denied.

## Automatic cleanup

```bash
make fix
```

This applies Rust formatting and safe Clippy suggestions, then reruns the quality gate.

## Dependency updates

Review dependency changes rather than updating blindly:

```bash
cargo update --dry-run
cargo tree
```

After accepting updates, run `make quality` and manually test folder scanning, concurrent encoding, cancellation, and contact-sheet generation.

## Release checklist

1. Update `CHANGELOG.md`.
2. Update the package version in `Cargo.toml`.
3. Run `make quality`.
4. Run `make release`.
5. Launch the release executable and perform a small real encode.
6. Verify that no source file is modified.
7. Verify cancellation removes partial output.
8. Tag the release in version control.

## Diagnosing warnings

- `cargo tree -i <crate>` identifies which dependency introduced a crate.
- `cargo report future-incompatibilities` explains warnings saved during a build.
- `cargo clippy --all-targets --all-features -- -D warnings` ensures warnings cannot be overlooked.
