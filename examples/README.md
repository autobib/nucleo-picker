# Picker examples
This directory contains a variety of examples of how to use the [nucleo-picker](https://docs.rs/nucleo-picker/latest/nucleo_picker/) crate in practice.

In order to try out the examples, run
```
cargo run --release --example <name>
```
where `<name>` is the part of the path without the `.rs` suffix.

Some of the examples may require arguments or feature flags to run properly; see the individual files for more information.

## Directory

File                       | Description
---------------------------|------------
[blocking.rs](blocking.rs) | A basic blocking example with a very small number of matches.
[find.rs](find.rs)         | A basic [find](https://en.wikipedia.org/wiki/Find_(Unix)) implementation with fuzzy matching on resulting items.
[fzf.rs](fzf.rs)           | A simple [fzf](https://github.com/junegunn/fzf) clone which reads lines from STDIN and presents for matching.
[options.rs](options.rs)   | Some customization examples of the picker.
[serde.rs](serde.rs)       | Use `serde` to deserialize picker items from input.
