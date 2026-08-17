#!/usr/bin/env bash
set -euo pipefail

for ((i = 0; i < 24; i++)); do
    printf 'item-%02d' "$i"
    case $((i % 3)) in
        1) printf '\n  detail-%02d-a' "$i" ;;
        2) printf '\n  detail-%02d-a\n  detail-%02d-b\n  detail-%02d-c' "$i" "$i" "$i" ;;
    esac
    printf '\0'
done
