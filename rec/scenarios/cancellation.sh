#!/usr/bin/env bash
set -euo pipefail
if (($# != 0)); then echo "usage: $0" >&2; exit 2; fi
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/common.sh"

start_fzf "$REC_DIR/fixtures/lines.txt" --no-multi
wait_complete 24 24
tt type item-2
wait_complete 6 24
snap cancellation-filtered
tt press Escape
finish_with_code 1
assert_output ''
