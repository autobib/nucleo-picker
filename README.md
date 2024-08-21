[![Current crates.io release](https://img.shields.io/crates/v/nucleo-picker)](https://crates.io/crates/nucleo-picker)
[![Documentation](https://img.shields.io/badge/docs.rs-nucleo--picker-66c2a5?labelColor=555555&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K)](https://docs.rs/nucleo-picker/)

# nucleo-picker
Yet another fuzzy picker library.
This library provides a TUI for the [`nucleo`](https://docs.rs/nucleo/latest/nucleo/) crate, but otherwise attempts to be a relatively thin wrapper.

As a result, you get the [great features](https://github.com/helix-editor/nucleo) of `nucleo` for free™.
This crate tries not to interfere with the API choices made by `nucleo`.

See the [`examples`](/examples) directory for implementation examples, or try it out with `cargo run --example blocking`.

Currently, we only directly load two dependencies:
- [nucleo](https://docs.rs/nucleo/latest/nucleo/) for matching
- [crossterm](https://docs.rs/crossterm/latest/crossterm/) for the interface

## Related crates
This crate mainly exists as a result of the author's annoyance with pretty much every fuzzy picker TUI in the rust ecosystem.
- [skim](https://docs.rs/skim/latest/skim/)'s `Arc<dyn SkimItem>` API is very inconvenient.
  `skim` also contains a large amount of dependency baggage.
- [fuzzypicker](https://docs.rs/fuzzypicker/latest/fuzzypicker/) is based on `skim` and inherits `skim`'s problems.
- [nucleo-ui](https://docs.rs/nucleo-ui/latest/nucleo_ui/) only has a blocking API and only supports matching on `String`.
- [fuzzy-select](https://docs.rs/fuzzy-select/latest/fuzzy_select/) only has a blocking API.
- [dialoguer `FuzzySelect`](https://docs.rs/dialoguer/latest/dialoguer/struct.FuzzySelect.html) only has a blocking API and only supports matching on `String`.
  The terminal handling also has a few strange bugs.

## Disclaimer
The feature set of this library is quite minimal (by design) but may be expanded in the future. There are a currently a few known problems which have not been addressed (see the [issues page on GitHub](https://github.com/autobib/nucleo-picker/issues) for a list).

This crate is not affiliated with the authors of `nucleo`, but if they have comments / complaints I am very glad to hear them!
