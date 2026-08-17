#!/usr/bin/env bash
set -euo pipefail
if (($# != 0)); then echo "usage: $0" >&2; exit 2; fi

REC_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
"$REC_DIR/scenarios/multiline/run.sh" default
"$REC_DIR/scenarios/multiline/run.sh" reverse
