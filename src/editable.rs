use std::fmt::{Display, Formatter};

/// The movement mode.
#[derive(Debug, PartialEq, Eq)]
pub enum MovementType {
    Left,
    Right,
    Start,
    End,
}

// TODO: update the internal type to be something that is has width exactly 1, since a char does not
// correspond to the width of a single column on the terminal.
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

    /// Paste a string at the current cursor position and return whether or not the string was
    /// changed.
    pub fn paste(&mut self, s: &str) -> bool {
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

    /// Insert a new char at the current cursor position and return whether or not the string was
    /// changed.
    pub fn insert(&mut self, ch: char) -> bool {
        self.contents.insert(self.cursor, ch);
        self.cursor += 1;
        true
    }

    /// Delete the character immediately preceding the cursor and return whether or not the string
    /// was changed.
    pub fn delete(&mut self) -> bool {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.contents.remove(self.cursor);
            true
        } else {
            false
        }
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

    /// Change the cursor position and return whether or not the cursor moved.
    #[inline]
    pub fn shift(&mut self, st: MovementType) -> bool {
        match st {
            MovementType::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    true
                } else {
                    false
                }
            }
            MovementType::Right => self.shift_to(self.cursor + 1),
            MovementType::Start => self.shift_to(0),
            MovementType::End => self.shift_to(self.contents.len()),
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
    fn test_edit() {
        let mut editable = EditableString::default();
        editable.insert('a');
        editable.insert('b');
        editable.insert('c');
        editable.insert('d');
        editable.shift(MovementType::Left);
        editable.insert('1');
        editable.insert('2');
        editable.insert('3');
        editable.shift(MovementType::Start);
        editable.delete();
        editable.insert('4');
        editable.shift(MovementType::End);
        editable.delete();

        assert_eq!(&editable.to_string(), "4abc123");
    }
}
