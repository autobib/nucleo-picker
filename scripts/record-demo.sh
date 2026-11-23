#!/usr/bin/env bash

set -euo pipefail

if (($# == 0)); then
    printf 'usage: %s example [example ...]\n' "$0" >&2
    exit 2
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

examples=()
for example in "$@"; do
    if [[ ! "$example" =~ ^[a-zA-Z0-9_-]+$ || ! -f "scripts/record-demo/$example.exp" ]]; then
        printf 'unknown example: %s\n' "$example" >&2
        exit 2
    fi
    if [[ " ${examples[*]-} " != *" $example "* ]]; then
        examples+=("$example")
    fi
done

for example in "${examples[@]}"; do
    cargo build --locked --release --all-features --example "$example" --target-dir target
done

export TERM=xterm-256color
mkdir -p target/record-demo

# Job control gives each recorder a process group for cleanup.
set -m

# shellcheck disable=SC2329
cleanup() {
    local pid
    for pid in $(jobs -p); do
        kill -- "-$pid" 2>/dev/null || true
    done
    wait || true
}
trap 'cleanup' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

pids=()
for example in "${examples[@]}"; do
    (
        output="target/record-demo/$example.cast"
        expect "scripts/record-demo/$example.exp" "$output"
        printf '%s\n' "$output"
    ) &
    pids+=("$!")
done

status=0
for pid in "${pids[@]}"; do
    wait "$pid" || status=$?
done
exit "$status"
