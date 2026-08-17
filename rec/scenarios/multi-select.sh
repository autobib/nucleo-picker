#!/usr/bin/env bash
set -euo pipefail
if (($# != 0)); then echo "usage: $0" >&2; exit 2; fi
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/common.sh"

start_fzf "$REC_DIR/fixtures/lines.txt" --multi 3
wait_complete 24 24
tt write $'\e[Z'
wait_queued 01 1
snap multi-select-one
tt write $'\e[Z'
wait_queued 02 2
snap multi-select-two
tt write $'\e[Z'
wait_queued 03 3
snap multi-select-three
tt press Enter
finish_with_code 0
assert_output $'item-00 alpha\nitem-01 bravo\nitem-02 charlie\n'
