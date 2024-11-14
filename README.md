[![Current crates.io release](https://img.shields.io/crates/v/nucleo-picker)](https://crates.io/crates/nucleo-picker)
[![Documentation](https://img.shields.io/badge/docs.rs-nucleo--picker-66c2a5?labelColor=555555&logoColor=white&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K)](https://docs.rs/nucleo-picker/)

> [!CAUTION]
> This the README for the forthcoming `v0.6.0` release, which includes a number of breaking changes to the API.
> These changes are required for more robust internal implementation and to resolve some outstanding issues.
> You can opt-in to bleeding edge changes by including the following line in the `[dependencies]` section of your `Cargo.toml`:
> ```
> nucleo-picker = { git = "https://github.com/autobib/nucleo-picker", branch = "master" }
> ```
> Any comments or issues, especially concerning API ergonomics and usability and Unicode handling, are very welcome!

# nucleo-picker
Yet another fuzzy picker library.
This library provides a TUI for the [`nucleo`](https://docs.rs/nucleo/latest/nucleo/) crate, but otherwise attempts to be a relatively thin wrapper.

The picker interface is similar to the very popular [fzf](https://github.com/junegunn/fzf) command-line tool, but rather than act as a standalone binary, this provides a [Rust](https://www.rust-lang.org/) library which allows you to incorporate a picker interface into your own application.

See the [`examples`](/examples) directory for implementation examples, or try out the sample `find/fzf` implementation by cloning the repository and running `cargo run --release --example find ~`.

## Features
These are some key features as to why you might want to use this crate over other alternatives.
Many of these features are courtesy of [`nucleo`](https://docs.rs/nucleo/latest/nucleo/).
- Highly optimized matching.
- Robust Unicode handling using [Unicode text segmentation](https://crates.io/crates/unicode-segmentation) and [Unicode width](https://crates.io/crates/unicode-width).
- Convenient API:
  - Non-blocking to match live on an item stream.
  - Generic `Picker` for any type `T` which is `Send + Sync + 'static`.
  - Stateful rendering of crate-local and foreign types with the `Render` trait.

## Query syntax
The query syntax is as documented in the [nucleo-matcher](https://docs.rs/nucleo-matcher/latest/nucleo_matcher/pattern/enum.AtomKind.html) crate.

Essentially, each query is parsed as a sequence of whitespace-separated "atoms", such as `a1 a2 a3`.
By default, each atom corresponds to a fuzzy match: that is, higher score is assigned for a closer match, but exact match is not required.
There is also a special syntax for various types of exact matches.

- `'foo` match an exact substring, with negation `!foo`
- `^foo` must match an exact prefix, with negation `!^foo`
- `foo$` must match an exact suffix, with negation `!foo$`
- `^foo$` must match the entire string exactly, with negation `!^foo$`

Whitespace and control symbols `'^$!` can also be interpreted literally by escaping with a backslash `\`.

For example, the query `foo ^bar` means that we match for strings which contain `foo` (or similar), and which begin with the exact string `bar`.

## Related crates
This crate mainly exists as a result of the author's annoyance with pretty much every fuzzy picker TUI in the rust ecosystem.
- [skim](https://docs.rs/skim/latest/skim/)'s `Arc<dyn SkimItem>` is very inconvenient since the original item cannot be recovered from the match.
  `skim` also contains a large amount of dependency baggage.
- [fuzzypicker](https://docs.rs/fuzzypicker/latest/fuzzypicker/) is based on `skim` and inherits `skim`'s problems.
- [nucleo-ui](https://docs.rs/nucleo-ui/latest/nucleo_ui/) only has a blocking API and only supports matching on `String`.
- [fuzzy-select](https://docs.rs/fuzzy-select/latest/fuzzy_select/) only has a blocking API.
- [dialoguer `FuzzySelect`](https://docs.rs/dialoguer/latest/dialoguer/struct.FuzzySelect.html) only has a blocking API and only supports matching on `String`.
  The terminal handling also has a few strange bugs.

## Disclaimer
The feature set of this library is quite minimal (by design) but may be expanded in the future. There are a currently a few known problems which have not been addressed (see the [issues page on GitHub](https://github.com/autobib/nucleo-picker/issues) for a list).

This crate is not affiliated with the authors of `nucleo`, but if they have comments / complaints I am very glad to hear them!
