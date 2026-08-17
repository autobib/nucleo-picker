#!/usr/bin/env bash
set -euo pipefail
# This is a private parameterized implementation, invoked by ../multiline.sh.
REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
# shellcheck disable=SC1091
source "$REC_DIR/common.sh"

layout=$1
case "$layout" in
    default) prefix=multiline-default; forward=Up; backward=Down ;;
    reverse) prefix=multiline-reverse; forward=Down; backward=Up ;;
    *) echo "invalid multiline layout: $layout" >&2; exit 2 ;;
esac

start_multiline "$layout"
wait_complete 24 24
snap "$prefix-initial-60x16"

for rows in 5 4 3 2 1; do
    tt resize 60 "$rows"
    if ((rows < 3)); then
        tt press Left
        tt wait idle --timeout 5000
    else
        wait_complete 24 24
    fi
    snap "$prefix-height-60x$rows"
done
tt resize 60 16
wait_complete 24 24

for cols in 5 4 3 2 1; do
    tt resize 60 16
    wait_complete 24 24
    tt resize "$cols" 16
    case $cols in
        5) tt wait text "24 (0" --timeout 5000 ;;
        4) tt wait text "/24 " --timeout 5000 ;;
        3) tt wait text "4/2" --timeout 5000 ;;
        2) tt wait text "/2" --timeout 5000 ;;
        1)
            if [[ $layout == reverse ]]; then
                tt wait idle --timeout 5000
            else
                tt wait text "/" --timeout 5000
            fi
            ;;
    esac
    snap "$prefix-width-${cols}x16"
done
tt resize 1 1
tt resize 60 16
wait_complete 24 24

tt resize 12 40
wait_complete 24 24
for ((i = 0; i < 14; i++)); do tt press "$forward"; done
for item in 14 15 16 17 18; do
    wait_selected "$item"
    snap "$prefix-narrow-12x40-item-$item"
    [[ $item == 18 ]] || tt press "$forward"
done
for item in 17 16 15 14; do
    tt press "$backward"
    wait_selected "$item"
    snap "$prefix-narrow-12x40-reverse-item-$item"
done

for ((i = 0; i < 14; i++)); do tt press "$backward"; done
tt resize 160 4
tt wait text "────" --timeout 5000
for item in 00 01 02 03 04; do
    wait_selected "$item"
    snap "$prefix-wide-160x4-item-$item"
    [[ $item == 04 ]] || tt press "$forward"
done
for item in 03 02 01; do
    tt press "$backward"
    wait_selected "$item"
    snap "$prefix-wide-160x4-reverse-item-$item"
done

tt resize 60 16
tt press "$forward"
wait_selected 02
tt press "$backward"
wait_selected 01
wait_complete 24 24
snap "$prefix-restored-60x16"
tt press Escape
finish_with_code 1
assert_output ''
