# Picker examples
This directory contains a variety of examples of how to use the [nucleo-picker](https://docs.rs/nucleo-picker/latest/nucleo_picker/) crate in practice.

In order to try out the examples, run
```
cargo run --release --example <name>
```
where `<name>` is the part of the path without the `.rs` suffix.

Some of the examples may require arguments or feature flags to run properly; see the individual files for more information.

## Extended `fzf` example

There is an extended `fzf` example [available separately](https://github.com/alexrutar/nucleo-picker-fzf).
Visit that repository for an in-depth illustration of how to use most features of this library.
Examples with smaller scope are listed here.

## Directory

File                                         | Description
---------------------------------------------|-------------
[blocking.rs](blocking.rs)                   | A basic blocking example with a very small number of matches.
[preview.rs](preview.rs)                     | A minimal blocking example with a preview pane.
[custom_io.rs](custom_io.rs)                 | Customize IO with keybindings and alternative writer.
[find.rs](find.rs)                           | A basic [find](https://en.wikipedia.org/wiki/Find_(Unix)) implementation with fuzzy matching on resulting items.
[find_preview.rs](find_preview.rs)           | A find impementation with asynchronous file previews.
[find_preview_full.rs](find_preview_full.rs) | A version of the `find_preview` example but with a detailed `Preview` implementation.
[fzf_basic.rs](fzf_basic.rs)                 | A simplified [fzf](https://github.com/junegunn/fzf) clone which presents lines from STDIN for matching.
[fzf_err_handling.rs](fzf_err_handling.rs)   | A simplified version of the `fzf` example using channels to propagate read errors.
[multi.rs](multi.rs)                         | A basic example allowing multiple selections.
[options.rs](options.rs)                     | Some customization examples of the picker.
[restart.rs](restart.rs)                     | Demonstration of interactive restarting in response to user input.
[restart_ext.rs](restart_ext.rs)             | An extended version of the `restart` example.
[timeout.rs](timeout.rs)                     | A version of the `find` example with an inactivity timeout.
[serde.rs](serde.rs)                         | Use `serde` to deserialize picker items from input.
[preview_serde.rs](preview_serde.rs)         | A blocking example with a preview pane, using `serde`.
[low_framerate.rs](low_framerate.rs)         | An example with a framerate of 0.5 FPS to demonstrate keypress input batching.
