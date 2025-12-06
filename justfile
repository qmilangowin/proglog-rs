# Justfile for proglog-rs - Distributed Log Service in Rust

# Default recipe that lists available commands
default:
    @just --list

# Build the project
build:
    cargo build

# Run cargo check
check:
    cargo check

# Build in release mode
build-release:
    cargo build --release

# Run all tests with basic logging
test:
    RUST_LOG=info cargo test

# Run tests with debug logging (verbose)
test-debug:
    RUST_LOG=debug cargo test -- --nocapture

# Run tests with store-specific logging
test-store:
    RUST_LOG=proglog_rs::storage::store=debug cargo test -- --nocapture

# Run tests with JSON structured logging
test-json:
    RUST_LOG=debug cargo test -- --nocapture --format json

# Run a specific test with debug logging
test-one TEST:
    RUST_LOG=debug cargo test {{TEST}} -- --nocapture

# Run tests and watch for file changes
test-watch:
    RUST_LOG=info cargo watch -x test

# Check code formatting
fmt-check:
    cargo fmt --all -- --check

# Format code
fmt:
    cargo fmt --all

# Run clippy lints
clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# Run clippy with fixes
clippy-fix:
    cargo clippy --fix --all-targets --all-features

# Clean build artifacts
clean:
    cargo clean

# Generate and open documentation
docs:
    cargo doc --open --no-deps

bacon:
    bacon

# Run benchmarks (when we add criterion)
bench:
    cargo bench

# Full CI check (format, clippy, test)
ci: fmt-check clippy test

# Development setup - install tools
dev-setup:
    cargo install just
    cargo install cargo-watch
    rustup component add clippy rustfmt

# Install pre-commit hooks
pre-commit-install:
    pre-commit install

# Run pre-commit on all files
pre-commit:
    pre-commit run --all-files

# Quick development cycle - format, clippy, test
dev: fmt clippy test

# Run the server (when we build it)
run *ARGS:
    RUST_LOG=info cargo run -- {{ARGS}}

# Run the server in debug mode
run-debug *ARGS:
    RUST_LOG=debug cargo run -- {{ARGS}}
