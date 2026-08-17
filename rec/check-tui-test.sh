#!/usr/bin/env bash
set -euo pipefail

REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
VERSION_FILE="$REC_DIR/TUI_TEST_VERSION"

command -v tui-test >/dev/null || { echo "tui-test is required" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }
if [[ ! -f $VERSION_FILE ]]; then
    echo "missing tui-test version pin: $VERSION_FILE" >&2
    exit 2
fi
expected=$(<"$VERSION_FILE")
actual=$(tui-test --version)
if [[ $actual != "$expected" ]]; then
    printf 'tui-test version mismatch\n  expected: %s\n  actual:   %s\n' \
        "$expected" "$actual" >&2
    exit 2
fi
