# `fzf` terminal recordings

This directory contains black-box integration tests for the release build of the `fzf` example.
The tests feed records through non-terminal stdin, drive the UI through a `tui-test` pseudo-terminal, compare full-screen color snapshots, and check process exit status and stdout.

Snapshots are not committed.
Each suite run records a baseline `before` Git revision and then generates and compares to an `after` revision or the current working directory if omitted.

## Usage

The suite requires Bash, Cargo, Git, jq, and the exact `tui-test` version recorded in `TUI_TEST_VERSION`.

Pass an optional baseline commit, tag, or branch and an optional comparison revision:

```bash
rec/diff.sh [BEFORE [AFTER]]
```

For example:

```bash
rec/diff.sh v0.11.1 HEAD
rec/diff.sh master next
rec/diff.sh HEAD~1 HEAD
rec/diff.sh HEAD
rec/diff.sh
```

With no arguments, `before` defaults to `HEAD` and `after` defaults to the current working directory.

Generate a report for one revision without comparing it to another:

```bash
rec/snapshot.sh [REV]
```

With no argument, `snapshot.sh` records the working tree. With a revision, it
records the resolved commit from an independent archive. It writes generated
terminal images to `rec/build/snapshot.html` and rendered terminal text to
`rec/build/snapshot.json`. The JSON does not contain color information.

With two arguments, the working tree is not checked out or used: `git archive` creates an independent temporary image of each revision.
With one argument, only `before` is archived and the `after` executable is built from the current, possibly dirty working directory.
Cargo runs once for each side, and the resulting executables are copied to:

- `build/fzf-before`, used to generate the baseline;
- `build/fzf`, used for the comparison.

Transient `.snap` files and baseline SVGs live under `build/baseline/`.
Routine command output is written to `build/run.log`; stage and scenario
progress, warnings, and errors remain visible in the terminal.

Every completed comparison writes a freestanding HTML report and a machine-readable JSON report:

```text
rec/build/report.html
rec/build/report.json
```

The HTML report always contains the compared revisions. If snapshots differ, the harness also captures each actual color-aware snapshot and SVG under `build/actual/` and embeds the SVG beside its baseline. A clean report contains only the heading and revision summary.

The JSON report has `before` and `after` revision fields and a `changed` array. `before` is the resolved commit hash; `after` is the resolved hash for an explicit comparison revision or `null` for the working tree. Each changed entry contains `scenario`, the literal `snapshot` name, complete `before_text` and `after_text` strings, and a `colors_changed` boolean. Color values themselves are not included.

## Recording syntax

The orchestration script exports the executable and transient snapshot paths, then runs the scenario scripts.
A scenario normally has this shape:

```bash
#!/usr/bin/env bash
set -euo pipefail
# shellcheck disable=SC1091
ROOT_MANIFEST=$(cargo locate-project --workspace --message-format plain)
source "${ROOT_MANIFEST%/Cargo.toml}/rec/common.sh"

start_fzf "$REC_DIR/fixtures/lines.txt" --no-multi
wait_complete 24 24
snap example-initial

tt type query
wait_complete 1 24
snap example-filtered

tt press Enter
finish_with_code 0
assert_output $'selected record\n'
```

Shared helpers include:

- `start_fzf FIXTURE [ARG ...]` starts the selected revision's copied binary at
  `60x16`, redirects the fixture to stdin, captures stdout, and records frame
  events in the scenario's temporary directory.
- `start_multiline [default|reverse]` generates the NUL-delimited fixture and
  starts with `--read0` and the requested layout.
- `tt COMMAND ...` invokes `tui-test` in the unique session.
  Common operations include `type`, `press`, `keys`, `write`, and `resize`.
- `wait_frame JQ_CONDITION [TIMEOUT_MS]` waits for a frame newer than the most
  recent input which satisfies the jq expression against its semantic fields.
- `wait_complete MATCHED TOTAL` waits for a completed match pass with the given
  counts.
- `wait_selected NN` waits for the item with the corresponding input index.
- `wait_queued NN COUNT` waits for the selected input index and queued count.
- `snap NAME` updates or compares the transient color-aware snapshot.
  Baseline mode also captures the corresponding real SVG.
- `finish_with_code CODE` waits for and checks command completion.
- `assert_output STRING` compares stdout byte-for-byte.

Use named keys such as `Enter`, `Escape`, `Up`, `Down`, `Left`, `Backspace`, or `Control+o` with `tt press`.
Use `tt write $'\e[Z'` for reverse-tab or another terminal sequence not represented by the named-key parser.

Always synchronize after input or resizing and before taking a snapshot.
Prefer `wait_frame` for model state and dimensions.
Text waits remain appropriate when the rendered text itself is the behavior under test.
Do not add arbitrary sleeps.

## Structure

- `diff.sh` resolves, archives, builds, and compares the two revisions.
- `snapshot.sh` records one revision without performing a comparison.
- `recordings.sh` discovers scenarios and runs them for both entrypoints.
- `prepare-fzf.sh` builds one archived revision and copies its release example.
- `check-tui-test.sh` enforces the exact CLI version in `TUI_TEST_VERSION`.
- `common.sh` provides session, synchronization, snapshot, output, report, and cleanup helpers.
- `build-diff.sh` collects real terminal SVG pairs in a standalone HTML report.
- `scenarios/*.sh` are auto-discovered public scenarios.
  They take no arguments.
- `scenarios/NAME/` may contain private scripts and other implementation details for a public `scenarios/NAME.sh` wrapper; nested scripts are not auto-discovered.
- `fixtures/` contains regular and generated NUL-delimited inputs.
- `build/` is ignored generated output.

Each recording uses a unique `tui-test` session and temporary output directory.
Cleanup traps close sessions and remove temporary files on success, failure, or interruption.
The launcher unsets `NO_COLOR` so highlighting and status colors remain part of the tested interface.

## Maintenance

Lint all shell scripts with:

```bash
find rec -name '*.sh' -print0 | xargs -0 shellcheck
```

When intentionally upgrading `tui-test`, update and verify the pin:

```bash
tui-test --version > rec/TUI_TEST_VERSION
rec/diff.sh HEAD~1 HEAD
```
