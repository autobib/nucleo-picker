#!/usr/bin/env bash
set -euo pipefail

if (($# > 1)); then
    echo "usage: $0 [REV]" >&2
    exit 2
fi

command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 2; }
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT_DIR=$(cd -- "$REC_DIR/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/recordings.sh"
BUILD_DIR="$REC_DIR/build"
SNAPSHOT_DIR="$BUILD_DIR/snapshot"
REPORT="$BUILD_DIR/snapshot.html"
JSON_REPORT="$BUILD_DIR/snapshot.json"
LOG="$BUILD_DIR/snapshot.log"
revision_ref=${1:-}

mkdir -p "$BUILD_DIR"
: >"$LOG"
export REC_LOG_PATH="$LOG"
exec 3>&1
exec >>"$LOG"
announce() {
    printf '%s\n' "$*" >&3
}
announce "initializing terminal snapshots (log: $LOG)"

"$REC_DIR/check-tui-test.sh"
command -v git >/dev/null || { echo "git is required" >&2; exit 2; }

archive_dir=
# Invoked indirectly by the trap below.
# shellcheck disable=SC2329
cleanup_archive() {
    if [[ -n $archive_dir ]]; then
        rm -rf -- "$archive_dir"
    fi
}
trap cleanup_archive EXIT HUP INT TERM

if [[ -n $revision_ref ]]; then
    revision_commit=$(git -C "$ROOT_DIR" rev-parse --verify --end-of-options \
        "$revision_ref^{commit}")
    revision_label=$revision_commit
    archive_dir=$(mktemp -d "${TMPDIR:-/tmp}/nucleo-picker-rec-revision.XXXXXX")
    git -C "$ROOT_DIR" archive "$revision_commit" | tar -x -C "$archive_dir"
    source_dir=$archive_dir
else
    revision_commit=
    revision_label="working tree"
    source_dir=$ROOT_DIR
fi

rm -rf -- "$SNAPSHOT_DIR"
rm -f -- "$REPORT" "$JSON_REPORT"
"$REC_DIR/build-diff.sh" snapshot-init "$REPORT" "$JSON_REPORT" \
    "$revision_commit" "$revision_label"

select_scenarios

"$REC_DIR/prepare-fzf.sh" "$source_dir" "$BUILD_DIR/fzf-snapshot"
mkdir -p "$SNAPSHOT_DIR"
export REC_TUI_TEST_READY=1 REC_SNAPSHOT_DIR="$SNAPSHOT_DIR"
export REC_FZF_PATH="$BUILD_DIR/fzf-snapshot" UPDATE_SNAPSHOTS=1
export REC_SNAPSHOT_REPORT_PATH="$REPORT"
export REC_SNAPSHOT_JSON_REPORT_PATH="$JSON_REPORT"

announce "recording snapshots: $revision_label"
recording_status=0
run_recordings snapshot 1 || recording_status=1
"$REC_DIR/build-diff.sh" finish "$REPORT"
exit "$recording_status"
