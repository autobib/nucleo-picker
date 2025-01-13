[![Current crates.io release](https://img.shields.io/crates/v/nucleo-picker)](https://crates.io/crates/nucleo-picker)
[![Documentation](https://img.shields.io/badge/docs.rs-nucleo--picker-66c2a5?labelColor=555555&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K)](https://docs.rs/nucleo-picker/)

# nucleo-picker
A native [Rust](https://www.rust-lang.org/) library which enables you to incorporate a highly performant and Unicode-aware fuzzy picker directly in your own terminal application.

This library provides a TUI for the [`nucleo`](https://docs.rs/nucleo/latest/nucleo/) crate with an interface similar to the [fzf](https://github.com/junegunn/fzf) command-line tool.

- For implementation examples, jump to the [fzf example](#example) or see the [`examples`](examples) directory.
- For documentation of interactive usage of the picker, see the [`USAGE.md`](USAGE.md) file.
- For a list of recent changes, see the [`CHANGELOG.md`](CHANGELOG.md) file.

## Elevator pitch
Why use this library instead of a general-purpose fuzzy-finder such as `fzf` or a lower level library such as `nucleo`?

1. **Much tighter integration between your data source and your application.**
   Instead of reading from a SQLite database with `sqlite3` and then parsing raw text, read directly into in-memory data structures with [`rusqlite`](https://docs.rs/rusqlite/latest/rusqlite/) and render the in-memory objects in the picker.
2. **Skip the subprocess overhead and improve startup time.**
   Instead of starting up a subprocess to call `fzf`, have the picker integrated directly into your binary.
3. **Distinguish items from their matcher representation.**
   Instead of writing your data structure to a string, passing it to `fzf`, and then parsing the resulting match string back into your data structure, directly obtain the original data structure when matching is complete.
4. **Don't spend time debugging terminal rendering edge cases.**
   Out-of-the-box, `nucleo-picker` handles terminal rendering subtleties such as *multiline rendering*, *double-width Unicode*, *automatic overflow scrollthrough*, and *grapheme-aware query input* so you don't have to.
5. **Handle support complex use cases using events.**
   `nucleo-picker` exposes a fully-featured [event system](https://docs.rs/nucleo-picker/latest/nucleo_picker/event/enum.Event.html) which can be used to drive the picker.
   This lets you [*customize keybindings*](https://docs.rs/nucleo-picker/latest/nucleo_picker/event/struct.StdinReader.html), support [*interactive restarts*](https://docs.rs/nucleo-picker/0.7.0-alpha.3/nucleo_picker/event/enum.Event.html#restart), and much more by implementing the `EventSource` trait.
   Simplified versions of such features are available in [fzf](https://github.com/junegunn/fzf) but essentially require manual configuration via an embedded DSL.

## Features
- [Highly optimized matching](https://github.com/helix-editor/nucleo).
- Robust rendering:
  - Full Unicode handling with [Unicode text segmentation](https://crates.io/crates/unicode-segmentation) and [Unicode width](https://crates.io/crates/unicode-width).
  - Match highlighting with automatic scroll-through.
  - Correctly render multi-line or overflowed items, with standard and reversed item order.
  - Responsive interface with batched keyboard input.
- Ergonomic API:
  - Fully concurrent lock- and wait-free streaming of input items.
  - Generic [`Picker`](https://docs.rs/nucleo-picker/latest/nucleo_picker/struct.Picker.html) for any type `T` which is `Send + Sync + 'static`.
  - [Customizable rendering](https://docs.rs/nucleo-picker/latest/nucleo_picker/trait.Render.html) of crate-local and foreign types with the `Render` trait.
- Fully configurable event system:
  - Easily customizable keybindings.
  - Run the picker concurrently with your application using a fully-featured [`Event` system](https://docs.rs/nucleo-picker/latest/nucleo_picker/event/enum.Event.html), with optional support for complex features such as [*interactive restarting*](https://docs.rs/nucleo-picker/0.7.0-alpha.3/nucleo_picker/event/enum.Event.html#restart).
  - Optional and flexible [error propagation generics](https://docs.rs/nucleo-picker/latest/nucleo_picker/event/enum.Event.html#application-defined-abort) so your application errors can interface cleanly with the picker.

## Example
Implement a heavily simplified `fzf` clone in 25 lines of code.
Try it out with:
```
cargo build --release --example fzf
cat myfile.txt | ./target/release/examples/fzf
```
The code to create the binary:
```rust
use std::{
    io::{self, IsTerminal},
    process::exit,
    thread::spawn,
};

use nucleo_picker::{render::StrRenderer, Picker};

fn main() -> io::Result<()> {
    let mut picker = Picker::new(StrRenderer);

    let injector = picker.injector();
    spawn(move || {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            for line in stdin.lines() {
                // silently drop IO errors!
                if let Ok(s) = line {
                    injector.push(s);
                }
            }
        }
    });

    match picker.pick()? {
        Some(it) => println!("{it}"),
        None => exit(1),
    }
    Ok(())
}
```


## Related crates

This crate mainly exists as a result of the author's annoyance with pretty much every fuzzy picker TUI in the rust ecosystem.
As far as I am aware, the fully-exposed [event system](https://docs.rs/nucleo-picker/latest/nucleo_picker/event/enum.Event.html) is unique to this crate.
Beyond this, here is a brief comparison:

- [skim](https://docs.rs/skim/latest/skim/)'s `Arc<dyn SkimItem>` is inconvenient for a [variety of reasons](https://rutar.org/writing/using-closure-traits-to-simplify-rust-api/).
  `skim` also has a large number of dependencies and is designed more as a binary than a library.
- [fuzzypicker](https://docs.rs/fuzzypicker/latest/fuzzypicker/) is based on `skim` and inherits `skim`'s problems.
- [nucleo-ui](https://docs.rs/nucleo-ui/latest/nucleo_ui/) only has a blocking API and only supports matching on `String`. It also seems to be un-maintained.
- [fuzzy-select](https://docs.rs/fuzzy-select/latest/fuzzy_select/) only has a blocking API.
- [dialoguer `FuzzySelect`](https://docs.rs/dialoguer/latest/dialoguer/struct.FuzzySelect.html) only has a blocking API and only supports matching on `String`.
  The terminal handling also has a few strange bugs.

## Disclaimer
There are a currently a few known problems which have not been addressed (see the [issues page on GitHub](https://github.com/autobib/nucleo-picker/issues) for a list). Issues and contributions are welcome!
