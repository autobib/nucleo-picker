# Changelog

## `v0.5.0`

- `Picker::new` has been deprecated; use `PickerOptions`.
- Improved interactive checks: now requires both stdin and stdout to be interactive.
- Various keybinding changes.
- Better exposure of nucleo internals:
  - Restart the internal matcher with `Picker::restart`
  - Update internal configuration without rebuilding the `Picker` using `Picker::update_config`
  - Modify the default query string using `Picker::update_query`
- New `PickerOptions` for more flexible `Picker` configuration:
  - Specifying an initial query string with `PickerOptions::query`
