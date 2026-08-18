#!/usr/bin/env bash
set -euo pipefail

if (($# != 2)); then
    echo "usage: $0 SOURCE_DIRECTORY OUTPUT" >&2
    exit 2
fi

source_dir=$1
output=$2
output_dir=${output%/*}
mkdir -p "$output_dir"
cargo build --locked --quiet --release --example fzf --features tracing \
    --manifest-path "$source_dir/Cargo.toml"
cp "$source_dir/target/release/examples/fzf" "$output"
