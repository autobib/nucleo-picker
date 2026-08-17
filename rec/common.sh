#!/usr/bin/env bash

if [[ -z ${BASH_VERSION:-} ]]; then
    echo "rec tests require Bash" >&2
    exit 2
fi

set -euo pipefail

REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
ROOT_DIR=$(cd -- "$REC_DIR/.." && pwd -P)
BUILD_DIR="$REC_DIR/build"
FZF=${REC_FZF_PATH:?REC_FZF_PATH must name the fzf executable under test}
SNAPSHOT_DIR=${REC_SNAPSHOT_DIR:?REC_SNAPSHOT_DIR must name the transient baseline directory}
cd "$REC_DIR"

if [[ ${REC_TUI_TEST_READY:-0} != 1 ]]; then
    "$REC_DIR/check-tui-test.sh"
fi

mkdir -p "$SNAPSHOT_DIR"

SESSION="nucleo-picker-$PPID-$$-$RANDOM"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/nucleo-picker-rec.XXXXXX")
OUTPUT="$TMP_DIR/stdout"
export TUI_TEST_SESSION="$SESSION"

cleanup() {
    tui-test close >/dev/null 2>&1 || true
    rm -rf -- "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM

tt() {
    tui-test "$@"
}

start_fzf() {
    local fixture=$1
    shift
    : >"$OUTPUT"
    tt open --shell bash --cols 60 --rows 16 --cwd "$ROOT_DIR"

    local command
    # Color and styling are part of the black-box contract. Do not let the
    # caller's NO_COLOR environment suppress the escape sequences under test.
    printf -v command 'unset NO_COLOR; %q' "$FZF"
    local arg
    for arg in "$@"; do
        printf -v command '%s %q' "$command" "$arg"
    done
    printf -v command '%s < %q > %q' "$command" "$fixture" "$OUTPUT"
    tt submit "$command"
}

start_multiline() {
    local layout=${1:-default}
    local fixture="$TMP_DIR/multiline"
    "$REC_DIR/fixtures/multiline.sh" >"$fixture"
    case $layout in
        default) start_fzf "$fixture" --read0 ;;
        reverse) start_fzf "$fixture" --read0 --reverse ;;
        *) echo "invalid multiline layout: $layout" >&2; return 2 ;;
    esac
}

wait_complete() {
    local matched=$1 total=$2
    # A completed match pass starts with the blank status marker. In-progress
    # frames use a spinner (or a middle dot) in that cell instead.
    tt wait text "  ${matched}/${total}" --timeout 10000
}

wait_selected() {
    local item=$1
    tt wait text "▌ item-$item" --timeout 5000
}

wait_queued() {
    local item=$1 count=$2
    wait_selected "$item"
    tt wait text "($count/3)" --timeout 5000
}

snap() {
    local args=(expect snapshot "$1" --include-colors)
    if [[ ${UPDATE_SNAPSHOTS:-0} == 1 ]]; then
        args+=(--update)
    fi
    local snapshot_status=0
    if [[ ${UPDATE_SNAPSHOTS:-0} == 1 || -z ${REC_LOG_PATH:-} ]]; then
        (cd "$SNAPSHOT_DIR" && tt "${args[@]}") || snapshot_status=$?
    else
        (cd "$SNAPSHOT_DIR" && tt "${args[@]}") 2>>"$REC_LOG_PATH" || snapshot_status=$?
    fi
    if ((snapshot_status != 0)); then
        if [[ ${UPDATE_SNAPSHOTS:-0} == 1 ]]; then
            return "$snapshot_status"
        fi
        build_diff_report "$1"
        return 0
    fi
    if [[ ${UPDATE_SNAPSHOTS:-0} == 1 ]]; then
        mkdir -p "$SNAPSHOT_DIR/svg"
        tt screenshot "$SNAPSHOT_DIR/svg/$1.svg"
    fi
}

build_diff_report() {
    local name=$1
    local expected="$SNAPSHOT_DIR/svg/$name.svg"
    local actual="$BUILD_DIR/after-$name.svg"
    local report=${REC_REPORT_PATH:-$BUILD_DIR/report.html}
    local failure_file=${REC_FAILURE_FILE:-$BUILD_DIR/report.fail}

    if [[ ! -f $expected ]]; then
        echo "missing baseline SVG: $expected" >&2
        return 2
    fi
    mkdir -p "$BUILD_DIR"
    rm -f -- "$actual"
    tt screenshot "$actual"
    "$REC_DIR/build-diff.sh" add "$expected" "$actual" "$report" "$name"
    rm -f -- "$actual"
    : >"$failure_file"
    echo "recorded snapshot failure: $name" >&2
}

assert_output() {
    local expected=$1
    local expected_file="$TMP_DIR/expected"
    printf '%s' "$expected" >"$expected_file"
    cmp "$expected_file" "$OUTPUT"
}

finish_with_code() {
    local code=$1
    tt wait command --timeout 10000
    tt expect exit-code "$code" --timeout 10000
}
