#!/usr/bin/env bash

set -euxo pipefail

export INSTA_UPDATE=new

manifest_path="$(cargo locate-project --message-format plain)"
cd "$(dirname "$manifest_path")"

cargo test --quiet --locked --no-run --all-features
cargo test --quiet --locked --no-fail-fast --all-features
cargo test --quiet --locked --no-fail-fast
cargo test --quiet --locked --release --package nucleo-picker-vt --no-default-features --no-run
cargo test --quiet --locked --release --package nucleo-picker-vt --no-default-features --no-fail-fast
cargo test --quiet --locked --release --package nucleo-picker-vt --all-features --no-run
cargo test --quiet --locked --release --package nucleo-picker-vt --all-features --no-fail-fast
cargo doc --quiet --locked --workspace --no-deps --all-features
cargo clippy --quiet --locked --workspace --all-targets --all-features
cargo fmt --all --check
