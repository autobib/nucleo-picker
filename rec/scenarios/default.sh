#!/usr/bin/env bash
set -euo pipefail
if (($# != 0)); then echo "usage: $0" >&2; exit 2; fi
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/common.sh"

start_fzf "$REC_DIR/fixtures/lines.txt" --no-multi
wait_complete 24 24
snap default-initial

tt type item-1
wait_complete 12 24
snap default-filtered
tt press Left Backspace
tt wait text "> item1" --timeout 5000
wait_complete 12 24
snap default-edited
tt type zzz
tt wait text "> itemzzz1" --timeout 5000
wait_complete 0 24
snap default-no-match
tt press Home Control+o
wait_complete 24 24
tt press Up Up Up
wait_selected 03
snap default-selection-03
tt press Down
wait_selected 02
snap default-selection-02
tt press Enter
finish_with_code 0
assert_output $'item-02 charlie\n'
