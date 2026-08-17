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
TRACE="$TMP_DIR/frames.jsonl"
FRAME_SEQUENCE=-1
export TUI_TEST_SESSION="$SESSION"

cleanup() {
    tui-test close >/dev/null 2>&1 || true
    rm -rf -- "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM

tt() {
    case ${1:-} in
        keys|press|resize|submit|type|write)
            FRAME_SEQUENCE=$(latest_frame_sequence)
            ;;
    esac
    tui-test "$@"
}

latest_frame_sequence() {
    jq -rs '
        map(select(.target == "nucleo_picker::frame"))
        | last.span.sequence // -1
    ' "$TRACE" 2>/dev/null || printf '%s\n' -1
}

start_fzf() {
    local fixture=$1
    shift
    : >"$OUTPUT"
    : >"$TRACE"
    FRAME_SEQUENCE=-1
    tt open --shell bash --cols 60 --rows 16 --cwd "$ROOT_DIR"

    local command
    # Color and styling are part of the black-box contract. Do not let the
    # caller's NO_COLOR environment suppress the escape sequences under test.
    printf -v command 'unset NO_COLOR; %q' "$FZF"
    local arg
    for arg in "$@"; do
        printf -v command '%s %q' "$command" "$arg"
    done
    printf -v command '%s --tracing-output %q' "$command" "$TRACE"
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
    wait_frame ".matching == false and .matched == $matched and .total == $total" 10000
}

wait_selected() {
    local item=$1
    local input_index=$((10#$item))
    wait_frame ".selected_input_index == $input_index"
}

wait_queued() {
    local item=$1 count=$2
    wait_selected "$item"
    wait_frame ".queued == $count"
}

wait_frame() {
    local condition=$1 timeout=${2:-5000}
    local deadline=$((SECONDS + (timeout + 999) / 1000))
    local filter="select(.target == \"nucleo_picker::frame\")
        | select(.span.sequence > $FRAME_SEQUENCE)
        | .span
        | select($condition)"

    while ((SECONDS <= deadline)); do
        if jq -e "$filter" "$TRACE" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.05
    done

    printf 'timed out waiting for frame condition after sequence %s: %s\n' \
        "$FRAME_SEQUENCE" "$condition" >&2
    local latest
    latest=$(jq -c 'select(.target == "nucleo_picker::frame") | .span' \
        "$TRACE" 2>/dev/null | tail -n 1) || true
    printf 'most recent frame: %s\n' "${latest:-<none>}" >&2
    return 1
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
        if [[ -n ${REC_SNAPSHOT_REPORT_PATH:-} ]]; then
            "$REC_DIR/build-diff.sh" snapshot-add "$SNAPSHOT_DIR/svg/$1.svg" \
                "$REC_SNAPSHOT_REPORT_PATH" "$REC_SNAPSHOT_JSON_REPORT_PATH" \
                "$REC_SCENARIO_NAME" "$1" "$SNAPSHOT_DIR/__snapshots__/$1.snap"
        fi
    fi
}

build_diff_report() {
    local name=$1
    local expected="$SNAPSHOT_DIR/svg/$name.svg"
    local scenario=${REC_SCENARIO_NAME:?REC_SCENARIO_NAME must name the current scenario}
    local actual_dir="$BUILD_DIR/actual/$scenario"
    local actual="$actual_dir/$name.svg"
    local actual_snapshot="$actual_dir/$name.snap"
    local expected_snapshot="$SNAPSHOT_DIR/__snapshots__/$name.snap"
    local report=${REC_REPORT_PATH:-$BUILD_DIR/report.html}
    local json_report=${REC_JSON_REPORT_PATH:-$BUILD_DIR/report.json}
    local failure_file=${REC_FAILURE_FILE:-$BUILD_DIR/report.fail}

    if [[ ! -f $expected ]]; then
        echo "missing baseline SVG: $expected" >&2
        return 2
    fi
    if [[ ! -f $expected_snapshot ]]; then
        echo "missing baseline snapshot: $expected_snapshot" >&2
        return 2
    fi
    mkdir -p "$actual_dir/__snapshots__"
    rm -f -- "$actual" "$actual_snapshot"
    tt screenshot "$actual"
    (cd "$actual_dir" && tt expect snapshot "$name" --include-colors --update)
    mv "$actual_dir/__snapshots__/$name.snap" "$actual_snapshot"
    "$REC_DIR/build-diff.sh" add "$expected" "$actual" "$report" "$json_report" \
        "$scenario" "$name" "$expected_snapshot" "$actual_snapshot"
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
