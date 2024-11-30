# Picker Interactive Usage
This file contains documentation for *using* the picker when it is actively running.

- [Query syntax](#query-syntax)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Scroll and paste](#scroll-and-paste)

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


## Keyboard shortcuts
Generally speaking, we attempt to follow the bash keyboard shortcut conventions.

Key bindings(s)         | Action
------------------------|--------------------
ctrl + c                | Abort
esc, ctrl + g, ctrl + q | Quit (no selection)
↑, ctrl + k, ctrl + p   | Selection Up
↓, ctrl + j, ctrl + n   | Selection Down
←, ctrl + b             | Cursor Left
→, ctrl + f             | Cursor Right
ctrl + a, ⇱             | Cursor To Start
ctrl + e                | Cursor To End
⌫, ctrl + h, shift + ⌫  | Backspace
⏎, shift + ⏎            | Select and Quit

## Scroll and paste
By default, the picker does not directly capture scroll actions, but if your terminal forwards scroll as up / down arrow input, then scrolling will work as expected.

Pasting is also not directly handled, but rather depends on whether or not your terminal handles [bracketed paste](https://en.wikipedia.org/wiki/Bracketed-paste).
If your terminal does not handle bracketed paste, then the characters are entered as though they were typed in one at a time, which may result in strange behaviour.
By default, input characters are normalized: newlines and tabs are replaced with spaces, and control characters are removed.
This is mainly relevant when pasting text into the query.
