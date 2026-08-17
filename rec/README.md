# `fzf` terminal recordings

This directory contains black-box integration tests for the release build of the `fzf` example.
The tests feed records through non-terminal stdin, drive the UI through a `tui-test` pseudo-terminal, compare full-screen color snapshots, and check process exit status and stdout.

Snapshots are not committed.
Each suite run records a baseline `before` Git revision and then generates and compares to an `after` revision or the current working directory if omitted.

## Usage

The suite requires Bash, Cargo, Git, and the exact `tui-test` version recorded
in `TUI_TEST_VERSION`.

Pass an optional baseline commit, tag, or branch and an optional comparison
revision:

```bash
rec/run.sh [BEFORE [AFTER]]
```

For example:

```bash
rec/run.sh v0.11.1 HEAD
rec/run.sh master next
rec/run.sh HEAD~1 HEAD
rec/run.sh HEAD
rec/run.sh
```

With no arguments, `before` defaults to `HEAD` and `after` defaults to the current working directory.

With two arguments, the working tree is not checked out or used: `git archive` creates an independent temporary image of each revision.
With one argument, only `before` is archived and the `after` executable is built from the current, possibly dirty working directory.
Cargo runs once for each side, and the resulting executables are copied to:

- `build/fzf-before`, used to generate the baseline;
- `build/fzf`, used for the comparison.

Transient `.snap` files and baseline SVGs live under `build/baseline/`.
Routine command output is written to `build/run.log`; stage and scenario
progress, warnings, and errors remain visible in the terminal.

If a snapshot comparison fails, the harness captures the actual terminal and embeds it beside the corresponding real baseline SVG.
The failures are collated into a single freestanding report:

```text
rec/build/report.html
```

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
  `60x16`, redirects the fixture to stdin, and captures stdout.
- `start_multiline [default|reverse]` generates the NUL-delimited fixture and
  starts with `--read0` and the requested layout.
- `tt COMMAND ...` invokes `tui-test` in the unique session. Common operations
  include `type`, `press`, `keys`, `write`, and `resize`.
- `wait_complete MATCHED TOTAL` waits for the blank completed-status marker and
  count, excluding spinner and middle-dot in-progress frames.
- `wait_selected NN` waits for the selected `item-NN` label.
- `wait_queued NN COUNT` waits for a selected label and queued count.
- `snap NAME` updates or compares the transient color-aware snapshot. Baseline
  mode also captures the corresponding real SVG.
- `finish_with_code CODE` waits for and checks command completion.
- `assert_output STRING` compares stdout byte-for-byte.

Use named keys such as `Enter`, `Escape`, `Up`, `Down`, `Left`, `Backspace`, or
`Control+o` with `tt press`. Use `tt write $'\e[Z'` for reverse-tab or another
terminal sequence not represented by the named-key parser.

Always synchronize after input or resizing and before taking a snapshot.
Prefer a completed match count, uniquely selected row, queued count, or
size-specific rendered fragment. Use `tt wait idle` only where the relevant
status cannot be observed. Do not add arbitrary sleeps.

## Structure

- `run.sh` resolves, archives, builds, and compares the two revisions.
- `prepare-fzf.sh` builds one archived revision and copies its release example.
- `check-tui-test.sh` enforces the exact CLI version in `TUI_TEST_VERSION`.
- `common.sh` provides session, synchronization, snapshot, output, report, and
  cleanup helpers.
- `build-diff.sh` collects real terminal SVG pairs in a standalone HTML report.
- `scenarios/*.sh` are auto-discovered public scenarios. They take no
  arguments.
- Every `scenarios/NAME.sh` requires a matching `scenarios/NAME.since`
  containing the first Git revision whose `fzf` binary supports the scenario.
  The scenario is skipped from both runs when that revision is not an ancestor
  of the baseline.
- `scenarios/NAME/` may contain private scripts and other implementation
  details for a public `scenarios/NAME.sh` wrapper; nested scripts are not
  auto-discovered.
- `fixtures/` contains regular and generated NUL-delimited inputs.
- `build/` is ignored generated output.

Each recording uses a unique `tui-test` session and temporary output directory.
Cleanup traps close sessions and remove temporary files on success, failure, or
interruption. The launcher unsets `NO_COLOR` so highlighting and status colors
remain part of the tested interface.

## Maintenance

Lint all shell scripts with:

```bash
find rec -name '*.sh' -print0 | xargs -0 shellcheck
```

When intentionally upgrading `tui-test`, update and verify the pin:

```bash
tui-test --version > rec/TUI_TEST_VERSION
rec/run.sh HEAD~1 HEAD
```
