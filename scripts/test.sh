#!/usr/bin/env bash

set -euxo pipefail

case "${MATRIX-full}" in
    no-features|all-features|full) ;;
    *)
        printf 'MATRIX=%q; allowed=no-features,all-features,full\n' "$MATRIX" >&2
        exit 2
        ;;
esac

manifest_path="$(cargo locate-project --workspace --message-format plain)"
cd "$(dirname "$manifest_path")"

cargo fmt --all --check

# job control gives each helper a process group for cleanup.
set -m

# shellcheck disable=SC2329
cleanup() {
    local pid
    for pid in $(jobs -p); do
        kill -- "-$pid" 2>/dev/null || true
    done
}
trap 'cleanup' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

./scripts/test_debug.sh &
debug_pid=$!
./scripts/test_vt.sh &
vt_pid=$!

status=0
wait "$debug_pid" || status=$?
wait "$vt_pid" || status=$?
exit "$status"
