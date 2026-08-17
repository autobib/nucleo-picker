#!/usr/bin/env bash
set -euo pipefail

snapshot_parts() {
    local snapshot=$1 prefix=$2 line total candidate
    total=$(wc -l <"$snapshot")
    for ((line = total; line >= 1; line--)); do
        candidate=$(sed -n "${line},\$p" "$snapshot")
        if jq -e 'type == "object" and (.colors | type == "object")' \
            <<<"$candidate" >/dev/null 2>&1; then
            head -n "$((line - 1))" "$snapshot" >"$prefix.text"
            jq '.colors' <<<"$candidate" >"$prefix.colors"
            return
        fi
    done
    cp "$snapshot" "$prefix.text"
    printf '%s\n' '{}' >"$prefix.colors"
}

command=${1:-}
case $command in
    init)
        if (($# != 6)); then
            echo "usage: $0 init OUTPUT.html OUTPUT.json BEFORE AFTER_COMMIT AFTER_LABEL" >&2
            exit 2
        fi
        output=$2
        json_output=$3
        before_revision=$4
        after_commit=$5
        after_revision=$6
        {
            printf '%s\n' '<!doctype html>'
            printf '%s\n' '<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
            printf '%s\n' '<title>fzf snapshot differences</title>'
            printf '%s\n' '<style>:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#17191d;color:#eee}body{margin:0;padding:24px}h1{margin-top:0}.revisions{display:grid;grid-template-columns:max-content 1fr;gap:6px 12px}.revisions dt{font-weight:700}.revisions dd{margin:0;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.difference{margin:32px 0 48px}.comparison{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}.panel{min-width:0}.terminal{overflow:auto;padding:12px;background:#101215;border-radius:8px}.terminal svg{display:block;max-width:none}@media(max-width:900px){.comparison{grid-template-columns:1fr}}</style></head><body>'
            printf '%s\n' '<h1>fzf snapshot differences</h1><main>'
            printf '<dl class="revisions"><dt>Before</dt><dd>%s</dd><dt>After</dt><dd>%s</dd></dl>\n' \
                "$before_revision" "$after_revision"
        } >"$output"
        if [[ -n $after_commit ]]; then
            jq -n --arg before "$before_revision" --arg after "$after_commit" \
                '{before: $before, after: $after, changed: []}' >"$json_output"
        else
            jq -n --arg before "$before_revision" \
                '{before: $before, after: null, changed: []}' >"$json_output"
        fi
        ;;
    add)
        if (($# != 9)); then
            echo "usage: $0 add BEFORE.svg AFTER.svg OUTPUT.html OUTPUT.json SCENARIO NAME BEFORE.snap AFTER.snap" >&2
            exit 2
        fi
        before=$2
        after=$3
        output=$4
        json_output=$5
        scenario=$6
        name=$7
        before_snapshot=$8
        after_snapshot=$9
        {
            printf '<section class="difference"><h2>%s</h2>\n' "$name"
            printf '%s\n' '<div class="comparison"><section class="panel"><h3>Before</h3><div class="terminal">'
            sed '/^<\?xml /d' "$before"
            printf '%s\n' '</div></section><section class="panel"><h3>After</h3><div class="terminal">'
            sed '/^<\?xml /d' "$after"
            printf '%s\n' '</div></section></div></section>'
        } >>"$output"

        parts_dir=$(mktemp -d "${TMPDIR:-/tmp}/nucleo-picker-rec-report.XXXXXX")
        cleanup_parts() {
            rm -rf -- "$parts_dir"
        }
        trap cleanup_parts EXIT
        snapshot_parts "$before_snapshot" "$parts_dir/before"
        snapshot_parts "$after_snapshot" "$parts_dir/after"
        colors_changed=false
        if ! jq -e --slurp '.[0] == .[1]' "$parts_dir/before.colors" \
            "$parts_dir/after.colors" >/dev/null; then
            colors_changed=true
        fi
        next_json="$parts_dir/report.json"
        jq --arg scenario "$scenario" --arg snapshot "$name" \
            --rawfile before_text "$parts_dir/before.text" \
            --rawfile after_text "$parts_dir/after.text" \
            --argjson colors_changed "$colors_changed" \
            '.changed += [{scenario: $scenario, snapshot: $snapshot, before_text: $before_text, after_text: $after_text, colors_changed: $colors_changed}]' \
            "$json_output" >"$next_json"
        mv "$next_json" "$json_output"
        rm -rf -- "$parts_dir"
        trap - EXIT
        ;;
    snapshot-init)
        if (($# != 5)); then
            echo "usage: $0 snapshot-init OUTPUT.html OUTPUT.json REVISION_COMMIT REVISION_LABEL" >&2
            exit 2
        fi
        output=$2
        json_output=$3
        revision_commit=$4
        revision_label=$5
        {
            printf '%s\n' '<!doctype html>'
            printf '%s\n' '<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
            printf '%s\n' '<title>fzf snapshots</title>'
            printf '%s\n' '<style>:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#17191d;color:#eee}body{margin:0;padding:24px}h1{margin-top:0}.revision{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.snapshot{margin:32px 0 48px}.terminal{overflow:auto;padding:12px;background:#101215;border-radius:8px}.terminal svg{display:block;max-width:none}</style></head><body>'
            printf '%s\n' '<h1>fzf snapshots</h1><main>'
            printf '<p class="revision">Revision: %s</p>\n' "$revision_label"
        } >"$output"
        if [[ -n $revision_commit ]]; then
            jq -n --arg revision "$revision_commit" \
                '{revision: $revision, snapshots: []}' >"$json_output"
        else
            jq -n '{revision: null, snapshots: []}' >"$json_output"
        fi
        ;;
    snapshot-add)
        if (($# != 7)); then
            echo "usage: $0 snapshot-add INPUT.svg OUTPUT.html OUTPUT.json SCENARIO NAME INPUT.snap" >&2
            exit 2
        fi
        image=$2
        output=$3
        json_output=$4
        scenario=$5
        name=$6
        snapshot=$7
        {
            printf '<section class="snapshot"><h2>%s: %s</h2><div class="terminal">\n' \
                "$scenario" "$name"
            sed '/^<\?xml /d' "$image"
            printf '%s\n' '</div></section>'
        } >>"$output"

        parts_dir=$(mktemp -d "${TMPDIR:-/tmp}/nucleo-picker-rec-snapshot.XXXXXX")
        cleanup_parts() {
            rm -rf -- "$parts_dir"
        }
        trap cleanup_parts EXIT
        snapshot_parts "$snapshot" "$parts_dir/snapshot"
        next_json="$parts_dir/report.json"
        jq --arg scenario "$scenario" --arg snapshot "$name" \
            --rawfile text "$parts_dir/snapshot.text" \
            '.snapshots += [{scenario: $scenario, snapshot: $snapshot, text: $text}]' \
            "$json_output" >"$next_json"
        mv "$next_json" "$json_output"
        rm -rf -- "$parts_dir"
        trap - EXIT
        ;;
    finish)
        if (($# != 2)); then
            echo "usage: $0 finish OUTPUT.html" >&2
            exit 2
        fi
        printf '%s\n' '</main></body></html>' >>"$2"
        ;;
    *)
        echo "usage: $0 {init|add|snapshot-init|snapshot-add|finish} ..." >&2
        exit 2
        ;;
esac
