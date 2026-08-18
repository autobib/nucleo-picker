# Picker interactive usage
This file contains documentation for interactive use of the picker.
Jump to:

- [Query syntax](#query-syntax)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Scroll and paste](#scroll-and-paste)


## Query syntax
The query is parsed as a sequence of whitespace-separated "atoms", such as `a1 a2 a3`.
By default, each atom corresponds to a fuzzy match: that is, higher score is assigned for a closer match, but exact match is not required.
There is also a special syntax for various types of exact matches.

- `'foo` matches an exact substring, with negation `!foo`
- `^foo` matches an exact prefix, with negation `!^foo`
- `foo$` matches an exact suffix, with negation `!foo$`
- `^foo$` matches the entire string exactly, with negation `!^foo$`

Note that the negations must match exactly.
The negation does not impact scoring: instead, any match for a negative atom is simply discarded, regardless of score.

Whitespace (that is, anything with the [Unicode whitespace property](https://www.unicode.org/Public/UCD/latest/ucd/PropList.txt)) and control symbols `'^$!` can also be interpreted literally by escaping with a backslash `\`.
Otherwise, backslashes are interpreted literally; in particular, backslashes do not need to be escaped.
For example:

- `\ ` matches the literal space ` `.
- `\\` and `\a` match, respectively, literal `\\` and `\a`.
- The query `fo\$ ^bar` means that we match for strings which contain `fo$` (or similar), and which begin with the exact string `bar`.


## Keyboard shortcuts
Generally speaking, we attempt to follow the bash-like or vim-like keyboard shortcut conventions.
Most of these bindings are relatively standard, with some exceptions like `ctrl + o` and `ctrl + r`.

Key bindings(s)                    | Action
-----------------------------------|--------------------
ctrl + c                           | Abort
⏎                                  | Select and Quit
esc, ctrl + g, ctrl + q            | Quit (no selection)
ctrl + d                           | Quit If Query Empty (no selection)
↑, ctrl + k, ctrl + p              | Selection Up
↓, ctrl + j, ctrl + n, shift + ⏎   | Selection Down
ctrl + 0                           | Reset Selection Scroll
←, ctrl + b                        | Cursor Left
→, ctrl + f                        | Cursor Right
ctrl + a, ⇱                        | Cursor To Start
ctrl + e                           | Cursor To End
ctrl + u                           | Clear Before Cursor
ctrl + o                           | Clear After Cursor
⌫, ctrl + h, shift + ⌫             | Backspace
ctrl + w                           | Backspace Word
␡, fn + ␡                          | Delete

There are also special keybindings which are only enabled in multi-selection mode.

Key bindings(s)                    | Action
-----------------------------------|--------------------
shift + ⇥, shift + ↑               | Toggle Queue And Selection Up
ctrl + =                           | Queue All Matches
ctrl + -                           | Unqueue All


## Scroll and paste
By default, the picker does not directly capture scroll actions, but if your terminal forwards scroll as up / down arrow input, then scrolling will work as expected.

Pasting is also not directly handled, but rather depends on whether or not your terminal handles [bracketed paste](https://en.wikipedia.org/wiki/Bracketed-paste).
If your terminal does not handle bracketed paste, then the characters are entered as though they were typed in one at a time, which may result in strange behaviour.
By default, input characters are normalized: newlines and tabs are replaced with spaces, and control characters are removed.
This is mainly relevant when pasting text into the query.

## Terminal support for Unicode
Unicode rendering is a difficult issue since the application and the terminal must agree on how much space each grapheme occupies on the screen.
The picker library uses [unicode-width](https://docs.rs/unicode-width/latest/unicode_width/#rules-for-determining-width); but old terminals without modern grapheme often use [wcwidth](https://man7.org/linux/man-pages/man3/wcwidth.3.html) or other bespoke solutions.
This can cause many rendering issues, such as line overflow and other types of screen corruption.

There is currently no check for mode 2027 support, but this may be added in the future for better terminal compatibility.
Terminal rendering currently works properly on [foot](https://codeberg.org/dnkl/foot), [contour](https://contour-terminal.org/), [ghostty](https://ghostty.org), and [wezterm](https://wezterm.org/index.html)
