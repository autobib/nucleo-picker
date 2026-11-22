#!/usr/bin/env bash

set -euxo pipefail

case "${MATRIX-full}" in
    no-features) feature_flags=(--no-default-features) ;;
    all-features) feature_flags=(--all-features) ;;
    full) feature_flags=(--no-default-features --all-features) ;;
    *)
        printf 'MATRIX=%q; allowed=no-features,all-features,full\n' "$MATRIX" >&2
        exit 2
        ;;
esac

export INSTA_UPDATE=new

manifest_path="$(cargo locate-project --workspace --message-format plain)"
cd "$(dirname "$manifest_path")"

for feature_flag in "${feature_flags[@]}"; do
    cargo test --quiet --locked --no-fail-fast --package nucleo-picker-vt --release "$feature_flag"
done
