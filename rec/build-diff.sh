#!/usr/bin/env bash
set -euo pipefail

command=${1:-}
case $command in
    init)
        if (($# != 4)); then
            echo "usage: $0 init OUTPUT.html BEFORE AFTER" >&2
            exit 2
        fi
        output=$2
        before_revision=$3
        after_revision=$4
        {
            printf '%s\n' '<!doctype html>'
            printf '%s\n' '<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
            printf '%s\n' '<title>fzf snapshot differences</title>'
            printf '%s\n' '<style>:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#17191d;color:#eee}body{margin:0;padding:24px}h1{margin-top:0}.revisions{display:grid;grid-template-columns:max-content 1fr;gap:6px 12px}.revisions dt{font-weight:700}.revisions dd{margin:0;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}.difference{margin:32px 0 48px}.comparison{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:20px}.panel{min-width:0}.terminal{overflow:auto;padding:12px;background:#101215;border-radius:8px}.terminal svg{display:block;max-width:none}@media(max-width:900px){.comparison{grid-template-columns:1fr}}</style></head><body>'
            printf '%s\n' '<h1>fzf snapshot differences</h1><main>'
            printf '<dl class="revisions"><dt>Before</dt><dd>%s</dd><dt>After</dt><dd>%s</dd></dl>\n' \
                "$before_revision" "$after_revision"
        } >"$output"
        ;;
    add)
        if (($# != 5)); then
            echo "usage: $0 add BEFORE.svg AFTER.svg OUTPUT.html NAME" >&2
            exit 2
        fi
        before=$2
        after=$3
        output=$4
        name=$5
        {
            printf '<section class="difference"><h2>%s</h2>\n' "$name"
            printf '%s\n' '<div class="comparison"><section class="panel"><h3>Before</h3><div class="terminal">'
            sed '/^<\?xml /d' "$before"
            printf '%s\n' '</div></section><section class="panel"><h3>After</h3><div class="terminal">'
            sed '/^<\?xml /d' "$after"
            printf '%s\n' '</div></section></div></section>'
        } >>"$output"
        ;;
    finish)
        if (($# != 2)); then
            echo "usage: $0 finish OUTPUT.html" >&2
            exit 2
        fi
        printf '%s\n' '</main></body></html>' >>"$2"
        ;;
    *)
        echo "usage: $0 {init|add|finish} ..." >&2
        exit 2
        ;;
esac
