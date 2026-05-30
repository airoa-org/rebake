# rebake-rs task runner
# Usage: just <task>

# Default: show available tasks
default:
    @just --list

# Format check (no changes)
fmt-check:
    cargo fmt --all -- --check

# Apply formatting
fmt:
    cargo fmt --all

# Python lint check
lint-python:
    uv tool run ruff check python/

# Python format check
fmt-check-python:
    uv tool run ruff format --check python/

# Python format
fmt-python:
    uv tool run ruff format python/

# Clippy lint
clippy:
    cargo clippy --workspace --all-targets --release -- -D warnings

# Build
build:
    cargo build --workspace --release

# Run Rust tests
test:
    cargo test --workspace --release

# Run Python tests
test-python:
    cd python && uv run --extra test pytest

# Run all tests (Rust + Python)
test-all: test test-python

# Run all checks (CI simulation)
check: fmt-check clippy test

# Run all checks including Python tests
check-all: fmt-check fmt-check-python lint-python clippy test-all
