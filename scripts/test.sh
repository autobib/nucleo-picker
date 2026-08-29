#!/usr/bin/env bash

set -euo pipefail

export INSTA_UPDATE=new

manifest_path="$(cargo locate-project --message-format plain)"
cd "$(dirname "$manifest_path")"

cargo test --locked --no-run --all-features
cargo test --locked --no-fail-fast --all-features
cargo test --locked --release --package nucleo-picker-vt --no-run
cargo test --locked --release --package nucleo-picker-vt --no-fail-fast
cargo doc --locked --workspace --no-deps --all-features
cargo clippy --locked --workspace --all-targets --all-features
cargo fmt --all --check
