#!/usr/bin/env bash
set -euo pipefail

if (($# > 2)); then
    echo "usage: $0 [BEFORE [AFTER]]" >&2
    exit 2
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 2; }
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT_DIR=$(cd -- "$REC_DIR/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/recordings.sh"
BUILD_DIR="$REC_DIR/build"
BASELINE_DIR="$BUILD_DIR/baseline"
REPORT="$BUILD_DIR/report.html"
JSON_REPORT="$BUILD_DIR/report.json"
FAILURE_FILE="$BUILD_DIR/report.fail"
LOG="$BUILD_DIR/run.log"
before_ref=${1:-HEAD}
after_ref=${2:-}

mkdir -p "$BUILD_DIR"
: >"$LOG"
export REC_LOG_PATH="$LOG"
exec 3>&1
exec >>"$LOG"
announce() {
    printf '%s\n' "$*" >&3
}
announce "initializing terminal recordings (log: $LOG)"

"$REC_DIR/check-tui-test.sh"
command -v git >/dev/null || { echo "git is required" >&2; exit 2; }

before_commit=$(git -C "$ROOT_DIR" rev-parse --verify --end-of-options "$before_ref^{commit}")
archive_dir=$(mktemp -d "${TMPDIR:-/tmp}/nucleo-picker-rec-revisions.XXXXXX")
# Invoked indirectly by the trap below.
# shellcheck disable=SC2329
cleanup_archives() {
    rm -rf -- "$archive_dir"
}
trap cleanup_archives EXIT HUP INT TERM

mkdir -p "$archive_dir/before"
git -C "$ROOT_DIR" archive "$before_commit" | tar -x -C "$archive_dir/before"

if [[ -n $after_ref ]]; then
    after_commit=$(git -C "$ROOT_DIR" rev-parse --verify --end-of-options "$after_ref^{commit}")
    mkdir -p "$archive_dir/after"
    git -C "$ROOT_DIR" archive "$after_commit" | tar -x -C "$archive_dir/after"
    after_source="$archive_dir/after"
    after_revision=$after_commit
else
    after_source=$ROOT_DIR
    after_revision="working tree"
fi

rm -f -- "$REPORT" "$JSON_REPORT" "$FAILURE_FILE"

select_scenarios

"$REC_DIR/prepare-fzf.sh" "$archive_dir/before" "$BUILD_DIR/fzf-before"
"$REC_DIR/prepare-fzf.sh" "$after_source" "$BUILD_DIR/fzf"

rm -rf -- "$BASELINE_DIR"
mkdir -p "$BASELINE_DIR"
export REC_TUI_TEST_READY=1 REC_SNAPSHOT_DIR="$BASELINE_DIR"

export REC_FZF_PATH="$BUILD_DIR/fzf-before" UPDATE_SNAPSHOTS=1
announce "recording baseline: $before_commit"
run_recordings baseline

"$REC_DIR/build-diff.sh" init "$REPORT" "$JSON_REPORT" "$before_commit" \
    "${after_commit:-}" "$after_revision"
export REC_FZF_PATH="$BUILD_DIR/fzf" UPDATE_SNAPSHOTS=0
export REC_REPORT_PATH="$REPORT" REC_JSON_REPORT_PATH="$JSON_REPORT"
export REC_FAILURE_FILE="$FAILURE_FILE"

announce "recording comparison: $after_revision"
comparison_status=0
run_recordings comparison 1 || comparison_status=1

"$REC_DIR/build-diff.sh" finish "$REPORT"
if [[ -f $FAILURE_FILE ]]; then
    echo "snapshot failure report: $REPORT" >&2
    comparison_status=1
fi
exit "$comparison_status"
