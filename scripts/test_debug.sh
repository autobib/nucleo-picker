#!/usr/bin/env bash

set -euxo pipefail

case "${MATRIX-full}" in
    no-features) feature_flag=--no-default-features ;;
    all-features) feature_flag=--all-features ;;
    full) feature_flag= ;;
    *)
        printf 'MATRIX=%q; allowed=no-features,all-features,full\n' "$MATRIX" >&2
        exit 2
        ;;
esac

export INSTA_UPDATE=new

manifest_path="$(cargo locate-project --workspace --message-format plain)"
cd "$(dirname "$manifest_path")"

test_package() {
    local package="$1"
    shift
    cargo test --quiet --locked --no-fail-fast --package "$package" "$@"
}

if [[ -n "$feature_flag" ]]; then
    for package in nucleo-picker-core nucleo-picker-interface nucleo-picker; do
        test_package "$package" "$feature_flag"
    done
else
    core_features=(serde preview-ansi ratatui crossterm)
    for ((mask = 0; mask < 16; mask++)); do
        features=""
        for ((bit = 0; bit < ${#core_features[@]}; bit++)); do
            if ((mask & (1 << bit))); then
                features="${features:+$features,}${core_features[bit]}"
            fi
        done
        test_package nucleo-picker-core --no-default-features --features "$features"
    done

    for features in "" preview-cache; do
        test_package nucleo-picker-interface --no-default-features --features "$features"
    done

    test_package nucleo-picker --all-features
    test_package nucleo-picker
    for features in "" serde preview-cache preview-ansi ratatui unstable-backend unstable-backend,preview-ansi; do
        test_package nucleo-picker --no-default-features --features "$features"
    done
fi

cargo doc --quiet --locked --workspace --no-deps --all-features
cargo clippy --quiet --locked --workspace --all-targets --all-features
