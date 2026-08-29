#!/usr/bin/env bash

set -euo pipefail

manifest_path="$(cargo locate-project --message-format plain)"
cd "$(dirname "$manifest_path")"

report_dir="$(mktemp -d)"
report_path="$report_dir/diff.html"

cargo run --quiet --locked --package nucleo-picker-vt --bin snap -- \
    --out "$report_path" \
    diff nucleo-picker-vt/tests/scenarios/snapshots

printf '%s\n' "$report_path"
