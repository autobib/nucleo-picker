# `nucleo-picker` virtual terminal integration tests

This crate implements integration tests for `nucleo-picker` using the excellent [`libghostty-vt`](https://docs.rs/libghostty-vt) library.
Running the code here requires a Zig compiler; see the [`libghostty-vt` docs](https://github.com/uzaaft/libghostty-rs#building) for more detail.

The virtual terminal setup and other generic helper code can be found in the library code in `src`.
The scenarios themselves are in the `tests/scenarios` folder.

## Terminal snapshots

The `nucleo-picker-vt` scenario tests use [Insta](https://insta.rs/) to store terminal pane snapshots in YAML.
Snapshots are produced by `checkpoint!` macro calls and committed under `tests/scenarios/snapshots`.

In order to review changes, it is most convenient to use the companion command `cargo insta`:

```bash
cargo install cargo-insta
```

### Snapshot files on disk

Each `checkpoint!` assertion has an associated `.snap` file with filename combing the test module and snapshot name.
For example:

```text
scenarios__default__basic@filtered.snap
```

THe `.snap` files are tracked by Git.
Each `.snap` file contains two YAML documents:

- A header records the source file, expression, resolved name, and original name expression, as well as a sequence number for the order from the original scenario.
- The second contains a serialized `PaneSnapshot`, including the viewport size, cursor, default colors, text, row flags, and style spans.

When a test result differs from its approved snapshot, Insta stores the complete replacement beside it with an additional `.new` suffix:

```text
scenarios__default__basic@filtered.snap      # baseline
scenarios__default__basic@filtered.snap.new  # pending replacement
```

New assertions initially have only a `.snap.new` file.
You can review all pending snapshots interactively:

```bash
cargo insta review --workspace
```

Skipped snapshots retain their `.snap.new` files and will be offered again the next time `cargo insta review` runs.
A completed change should leave no `.snap.new` files in the working tree.

### Generating changes

Run all scenario tests through Cargo Insta from the workspace root:

```bash
cargo insta test --package nucleo-picker-vt --test scenarios
```

Unlike a normal `cargo test` run, this collects all snapshot changes instead of stopping at the first snapshot assertion.
Changed and new snapshots are written as pending `.snap.new` files, and approved `.snap` files are not overwritten.

To run only one scenario, add its test-name filter:

```bash
cargo insta test --package nucleo-picker-vt --test scenarios unicode_scenario
```
