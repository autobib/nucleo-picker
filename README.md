# nucleo-picker
Yet another fuzzy picker library. This library provides a TUI for the [`nucleo`](https://docs.rs/nucleo/latest/nucleo/) crate, but otherwise attempts to be a relatively thin wrapper.

As a result, you get the great features of [`nucleo`](https://docs.rs/nucleo/latest/nucleo/) for free. This crate tries not to interfere with the API choices made by `nucleo`.

See the [`/examples`](/examples) directory for implementation examples or try it out with `cargo run --example blocking`.

We only directly load two dependencies:
- [nucleo](https://docs.rs/nucleo/latest/nucleo/) for matching
- [crossterm](https://docs.rs/crossterm/latest/crossterm/) for the interface

## Related crates
This crate mainly exists as a result of the author's annoyance with pretty much every fuzzy picker TUI in the rust ecosystem.
- [skim](https://docs.rs/skim/latest/skim/)'s `Arc<dyn SkimItem>` API is very inconvenient and also contains a large amount of dependency baggage.
- [fuzzypicker](https://docs.rs/fuzzypicker/latest/fuzzypicker/) is based `skim` and inherits `skim`'s problems.
- [nucleo-ui](https://docs.rs/nucleo-ui/latest/nucleo_ui/) only has a blocking API and only supports matching on `String`.
- [fuzzy-select](https://docs.rs/fuzzy-select/latest/fuzzy_select/) only has a blocking API.
- [dialoguer `FuzzySelect`](https://docs.rs/dialoguer/latest/dialoguer/struct.FuzzySelect.html) only has a blocking API and only supports matching on `String`. The terminal handling also has a few strange bugs.

## Disclaimer
The feature set of this library is quite minimal (by design) but may be expanded in the future. There are a currently a few known problems which have not been addressed (see the issues page on GitHub for a list).

This crate is not affiliated with the authors of [`nucleo`](https://docs.rs/nucleo/latest/nucleo/), but if they have comments / complaints I am very glad to hear them!
