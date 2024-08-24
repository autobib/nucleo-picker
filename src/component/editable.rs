use std::fmt::{Display, Formatter};

/// The movement mode.
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    /// Move the cursor left.
    MoveLeft,
    /// Move the cursor right.
    MoveRight,
    /// Move the cursor to the start.
    MoveToStart,
    /// Move the cursor to the end.
    MoveToEnd,
    /// Insert a char at the current cursor position.
    Insert(char),
    /// Delete a char immediately preceding the current cursor position.
    Delete,
    /// Paste a string at the current cursor position.
    Paste(String),
}

/// A simple editable string type with a cursor indicating the current edit position, and various
/// supported actions.
#[derive(Debug, Default)]
pub struct EditableString {
    /// The contents of the editable string.
    contents: Vec<char>,
    /// The position within the string.
    cursor: usize,
    /// Scratch space for operations such as non-append paste.
    _scratch: Vec<char>,
}

impl EditableString {
    /// The cursor position within the string.
    pub fn position(&self) -> usize {
        self.cursor
    }

    /// Is the cursor at the end of the string?
    pub fn cursor_at_end(&self) -> bool {
        self.cursor == self.contents.len()
    }

    /// Change the cursor position to the provided index return whether or not the cursor moved.
    #[inline]
    fn shift_to(&mut self, pos: usize) -> bool {
        if pos <= self.contents.len() && self.cursor != pos {
            self.cursor = pos;
            true
        } else {
            false
        }
    }

    /// Apply the provided [`Edit`].
    #[inline]
    pub fn edit(&mut self, st: Edit) -> bool {
        match st {
            Edit::MoveLeft => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    true
                } else {
                    false
                }
            }
            Edit::MoveRight => self.shift_to(self.cursor + 1),
            Edit::MoveToStart => self.shift_to(0),
            Edit::MoveToEnd => self.shift_to(self.contents.len()),
            Edit::Insert(ch) => {
                self.contents.insert(self.cursor, ch);
                self.cursor += 1;
                true
            }
            Edit::Paste(s) => {
                if !s.is_empty() {
                    // we are appending, so we can avoid some writes.
                    if self.cursor_at_end() {
                        self.contents.extend(s.chars());
                        self.cursor = self.contents.len();
                    } else {
                        // the new characters to append
                        self._scratch.clear();
                        self._scratch.extend(s.chars());
                        let extend_len = self._scratch.len();

                        // extend by the tail of the cursor
                        self._scratch.extend(self.contents.drain(self.cursor..));

                        // increment cursor and append the extension
                        self.cursor += extend_len;
                        self.contents.append(&mut self._scratch);
                    }
                    true
                } else {
                    false
                }
            }
            Edit::Delete => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.contents.remove(self.cursor);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl Display for EditableString {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let content_str: String = self.contents.iter().collect();
        content_str.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit() {
        let mut editable = EditableString::default();
        for e in [
            Edit::Insert('a'),
            Edit::Insert('b'),
            Edit::Insert('c'),
            Edit::Insert('d'),
            Edit::MoveLeft,
            Edit::Insert('1'),
            Edit::Insert('2'),
            Edit::Insert('3'),
            Edit::MoveToStart,
            Edit::Delete,
            Edit::Insert('4'),
            Edit::MoveToEnd,
            Edit::Delete,
        ] {
            editable.edit(e);
        }

        assert_eq!(&editable.to_string(), "4abc123");
    }
}
