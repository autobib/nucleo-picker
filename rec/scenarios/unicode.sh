#!/usr/bin/env bash
set -euo pipefail
if (($# != 0)); then echo "usage: $0" >&2; exit 2; fi
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/common.sh"

wait_query() {
    local query=$1
    wait_frame ".matching == false and .query == \"$query\""
}

clear_query() {
    tt press Control+u
    wait_query ''
}

start_fzf "$REC_DIR/fixtures/unicode.txt" --no-multi
wait_complete 7 7
tt resize 100 14
wait_frame '.width == 100 and .height == 14'
snap unicode-wide-initial

tt resize 20 12
wait_frame '.width == 20 and .height == 12'
snap unicode-narrow-right-elision

tt resize 20 3
wait_frame '.width == 20 and .height == 3'

tt type NFCcafé
wait_query NFCcafé
snap unicode-latin-nfc-highlight
clear_query

tt type NFDcafé
wait_query NFDcafé
snap unicode-latin-nfd-highlight
clear_query

tt type 한국어서울
wait_query 한국어서울
snap unicode-korean-highlight
clear_query

tt type 日本語東京
wait_query 日本語東京
snap unicode-japanese-highlight
clear_query

tt resize 20 12
wait_frame '.width == 20 and .height == 12'
tt type final
wait_query final
snap unicode-narrow-left-elision

tt resize 12 12
wait_frame '.width == 12 and .height == 12'
snap unicode-very-narrow-left-elision

tt press Escape
finish_with_code 1
assert_output ''
