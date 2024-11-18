# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **Breaking** `Picker` now requires new `Render` implementation to describe how a given type is rendered on the screen.
  - `Picker::new` signature has changed.
  - `PickerOptions::picker` signature has changed.
- **Breaking** `Picker::pick` uses STDERR instead of STDOUT for interactive screen. A lock is acquired to STDERR for rendering performance in case of Mutex contention.
  - If your application requires debug logging, it is probably best to log to a file instead.
- **Breaking** `Picker::injector` now returns a `nucleo_picker::Injector` instead of a `nucleo::Injector`. The `nucleo_picker::Injector` no longer exposes the internal match object; instead, rendering is done by the new `Render` trait.
- User CTRL-C during `Picker::pick` now returns `io::Error` with custom error message.

### Removed
- Suggested support for multiple columns has now been removed.

### Fixed
- Picker no longer blocks STDIN and STDOUT.
- Pressing DELETE when the prompt is empty no longer causes screen redraw.
- Correctly handle `\!`, `\^`, and `\$`.


### Added
- Match highlighting
- Robust Unicode and mutiline support
  - Correctly renders multi-line items
  - Unicode width computations to correctly handle double-width and zero-width graphemes.
- Convenient `Render` implementations in new `render` module.
- New configuration options for `PickerOptions`.
- New example: `fzf` clone

## [0.5.0] - 2024-11-7

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
