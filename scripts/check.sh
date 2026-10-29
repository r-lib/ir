#!/bin/sh

set -eu

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo nextest run --verbose --no-fail-fast
