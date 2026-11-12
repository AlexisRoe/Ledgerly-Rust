set shell := ["bash", "-euo", "pipefail", "-c"]

# List every task.
default:
    @just --list

# Everything CI checks, in order. Grows with the milestones (docs/local-setup.md §4).
verify: vendor-check fmt-check clippy test

# Fail if a file vendored from Ledgerly-Java was changed, removed or added (ADR 0007). Works offline.
vendor-check:
    scripts/vendor-check.sh

# Fail if any file is not formatted.
fmt-check:
    cargo fmt --all --check

# Format every crate.
fmt:
    cargo fmt --all

# Lint all targets; warnings are errors.
clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Unit and integration tests with nextest, then doc-tests (nextest does not run those).
test:
    cargo nextest run --workspace --locked
    cargo test --workspace --locked --doc
