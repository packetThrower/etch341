# etch341 build / test helpers.

# Default: full build (CLI + GUI).
default: build

# Build with the `gui` feature on (pulls GPUI from the Zed monorepo on
# first run — multi-minute fetch, subsequent builds are fast).
build:
    cargo build

# CLI-only build. Skips the GPUI dep tree entirely; small + fast.
build-cli:
    cargo build --no-default-features

# Release builds, same split.
release:
    cargo build --release

release-cli:
    cargo build --release --no-default-features

# Unit tests. Don't need hardware; mocks the SPI transport.
test:
    cargo test --no-default-features

# Hardware-in-the-loop tests. Requires a CH341A + chip in the ZIF
# socket. Tests behind `#[cfg(feature = "hardware")]` run only here.
test-hw:
    cargo test --features hardware

# Lint.
lint:
    cargo clippy --no-default-features -- -D warnings
    cargo clippy -- -D warnings

# Format.
fmt:
    cargo fmt

# Verify the tree is already formatted (CI-style; non-zero exit on drift).
fmt-check:
    cargo fmt --all -- --check

# Fast typecheck (no codegen). CLI-only path.
check:
    cargo check --no-default-features

# Run before committing: fmt drift + clippy + tests.
precommit: fmt-check lint test

# Run the GUI.
run:
    cargo run

# Run a CLI subcommand. Example: `just run-cli detect -v`
run-cli *args:
    cargo run --no-default-features -- {{args}}
