#!/usr/bin/env bash

scenario_scripts=()

select_scenarios() {
    shopt -s nullglob
    scenario_scripts=("$REC_DIR"/scenarios/*.sh)
    shopt -u nullglob
    if ((${#scenario_scripts[@]} == 0)); then
        echo "no scenario scripts found in $REC_DIR/scenarios" >&2
        return 2
    fi
}

run_recordings() {
    local phase=$1 keep_going=${2:-0} scenario status=0
    for scenario in "${scenario_scripts[@]}"; do
        REC_SCENARIO_NAME=$(basename -- "$scenario" .sh)
        export REC_SCENARIO_NAME
        announce "$phase scenario: $REC_SCENARIO_NAME"
        if ! "$scenario"; then
            status=1
            if ((keep_going == 0)); then
                return "$status"
            fi
        fi
    done
    return "$status"
}
