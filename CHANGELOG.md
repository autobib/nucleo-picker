# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Breaking** The underlying `nucleo` crate has been patched with the new [ncp-engine](https://docs.rs/ncp-engine) crate, and is now an implementation detail which will stop being public in `0.11.0`.
  In order to avoid confusion about the crate naming, the configuration options no longer expose the underlying `nucleo` instance.
  To migrate:
  - Use the native `nucleo_picker::PickerOptions::{prefer_prefix, match_paths}` methods.
  - Use the native `nucleo_picker::{CaseMatching, Normalization}` types in place of `nucleo::pattern::{CaseMatching, Normalization}`.
  - The `normalize` and `ignore_case` options are already ignored, so there is no replacement.
    You should instead use `PickerOptions::normalization` and `PickerOptions::case_matching`.

### Added
- New options for the sorting the match list, which are particularly useful when the desired behaviour is to *filter* rather than to *sort by score*.
  - `PickerOptions::sort_results`: which enables or disables sorting by score.
  - `PickerOptions::reverse_items`: prioritize higher index over lower index items.

### Fixed
- Fixed missing or incorrect Latin normalization

### Added
- Most configuration options are now available in `const` contexts.

## [0.9.0] - 2025-09-03
### Added
- Added method `Picker::query` to obtain the contents of the query string internal to the picker.

### Changed
- **Breaking** The `Picker::restart` method clears the query string.
- Migrated to Rust Edition 2024.

### Fixed
- Calling `Picker::update_query` correctly propagates the update to the internal matching engine.

## [0.8.1] - 2025-02-07

### Added
- Added method `Injector::renderer` to get a reference to the `Render` implementation internal to the picker.

## [0.8.0] - 2025-01-14

### Changed
- **Breaking** The `EventSource` trait method `recv_timeout` now takes a mutable self-reference.
  This is to allow an `EventSource` implementation to maintain internal state.

### Added
- Keybindings are now permitted to be `FnMut` rather than just `Fn`.

## [0.7.0] - 2025-01-13

### Changed
- **Breaking** `Picker::pick` now returns an `error::PickError` instead of an `io::Error`.
  The new error type is required to more faithfully represent the possible failure modes of a custom `EventSource` implementation.
  There is a `From<error::PickError> for io::Error` implementation to minimize breakage of existing code.
  However, the corresponding `io::Error::other` message contents have now changed to respect the new error types.

### Added
- Reset selection to beginning of match list `ctrl + 0`.
- New `PickerOptions::frame_interval` option to customize the refresh rate of the picker.
- Reversed rendering with `PickerOptions::reversed`
- New `Picker::pick_with_io` and `Picker::pick_with_keybind` functions that allows much greater IO customization.
  - Provide your own `Writer`.
  - Customize keybindings using a `StdinReader`.
  - Drive the picker using a `mpsc` channel.
  - Propagate custom errors to the picker from other threads.
  - Implement your own `EventSource` for total customization.
- Support for interactive restarting
  - Initialize a restart using `Event::Restart`.
  - Watch for new `Injector`s using the `Observer` returned by `Picker::injector_observer`.
- New examples to demonstrate the `Event` system
  - `custom_io` for a basic example
  - `fzf_err_handling` to use channels for event propagation
  - `restart` to demonstrate interactive restarting (with extended example `restart_ext`)

### Fixed
- Fixed screen layout when resizing to prefer higher score elements.
- Uses panic hook to correctly clean up screen if the picker panics.

## [0.6.4] - 2024-12-16

### Added
- The picker now quits on `ctrl + d` if the query is empty.
- Add "Backspace Word" on `ctrl + w`.

### Fixed
- Picker no longer quits when pressing 'Enter' with no matches

## [0.6.3] - 2024-12-11

### Fixed
- STDERR is now buffered to improve terminal write performance.
- Corrected docs to clarify that control characters should not be included in rendered text.

## [0.6.2] - 2024-12-11

### Added
- Added configuration for prompt padding and scroll padding.
- Added key-bindings to go forward and backward by word, and to clear before and after cursor.
- Support deleting next character (i.e. `Delete` on windows, and `fn + delete` on MacOS).

### Deprecated
- `PickerOptions::right_highlight_padding` has been deprecated; use `PickerOptions::highlight_padding` instead.

### Fixed
- Fixed highlight padding to correctly fill for highlight matches very close to the end of the screen.
- Proper handling of graphemes and multi-width characters in the prompt string (#4).
- Removed some unnecessary features from dependencies.

## [0.6.1] - 2024-12-04

### Added
- New implementation of `Render<T>` for any type which is `for<'a> Fn(&'a T) -> Cow<'a, str>`.
- Improved documentation.

## [0.6.0] - 2024-12-01

### Changed
- **Breaking** `Picker` now requires new `Render` implementation to describe how a given type is displayed on the screen.
  - `Picker::new` signature has changed.
  - `PickerOptions::picker` signature has changed.
- **Breaking** `PickerOptions::query` and `Picker::set_query` now accept any argument which is `Into<String>` instead of `ToString`.
- **Breaking** `Picker::pick` uses STDERR instead of STDOUT for interactive screen.
    A lock is acquired to STDERR to reduce the chance of rendering corruption and prevent Mutex contention.
  - If your application requires debug logging, it is probably best to log to a file instead.
- **Breaking** `Picker::injector` now returns a `nucleo_picker::Injector` instead of a `nucleo::Injector`. The `nucleo_picker::Injector` no longer exposes the internal match object; instead, rendering is done by the new `Render` trait.
- User CTRL-C during `Picker::pick` now returns `io::Error` with custom error message.

### Removed
- Suggested support for multiple columns has now been removed (multiple columns were never supported internally).

### Fixed
- Picker no longer blocks STDIN and STDOUT. (#15)
- Pressing DELETE when the prompt is empty no longer causes screen redraw.
- Use synchronized output to avoid screen tearing on large render calls. (#14)
- Correctly handle `\!`, `\^`, and `\$`.
- Query strings are now correctly normalized to replace newlines and tabs with single spaces, and to disallow ASCII control characters.

### Added
- Match highlighting. (#9)
- Robust Unicode and multi-line support
  - Correctly renders multi-line items
  - Unicode width computations to correctly handle double-width and zero-width graphemes.
- Full match item scrollback
- Convenient `Render` implementations in new `render` module.
- New configuration options for `PickerOptions`. (#2)
- New example: `fzf` clone
- Convenience features for adding new items to a `Picker`:
  - `Picker` and `Injector` now implement `Extend` for convenient item adding.
  - With the optional `serde` feature, an `&Injector` now implements `DeserializeSeed` to allow adding items from the picker directly from a deserializer.

## [0.5.0] - 2024-11-07

### Added
- Better exposure of nucleo internals:
  - Restart the internal matcher with `Picker::restart`
  - Update internal configuration without rebuilding the `Picker` using `Picker::update_config`
  - Modify the default query string using `Picker::update_query`
- New `PickerOptions` for more flexible `Picker` configuration:
  - Specifying an initial query string with `PickerOptions::query`

### Deprecated
- `Picker::new` has been deprecated; use `PickerOptions`.

### Changed
- Modified interactive checks: now requires both stdin and stdout to be interactive.
- Various keybinding changes.
