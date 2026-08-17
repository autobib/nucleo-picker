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
    wait_frame ".width == 60 and .height == $rows"
    snap "$prefix-height-60x$rows"
done
tt resize 60 16
wait_frame '.width == 60 and .height == 16'

for cols in 5 4 3 2 1; do
    if ((cols < 5)); then
        tt resize 60 16
        wait_frame '.width == 60 and .height == 16'
    fi
    tt resize "$cols" 16
    wait_frame ".width == $cols and .height == 16"
    snap "$prefix-width-${cols}x16"
done
tt resize 1 1
wait_frame '.width == 1 and .height == 1'
tt resize 60 16
wait_frame '.width == 60 and .height == 16'

tt resize 12 40
wait_frame '.width == 12 and .height == 40'
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
wait_frame '.width == 160 and .height == 4'
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
wait_frame '.width == 60 and .height == 16'
tt press "$forward"
wait_selected 02
tt press "$backward"
wait_selected 01
wait_complete 24 24
snap "$prefix-restored-60x16"
tt press Escape
finish_with_code 1
assert_output ''
