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

The query syntax is also documented in the [nucleo-matcher](https://docs.rs/nucleo-matcher/latest/nucleo_matcher/pattern/enum.AtomKind.html) crate.


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
