use std::{
    cmp::min,
    fmt::{Display, Formatter},
};

/// A representation of the current view state of an [`EditableString`] created by the
/// [`EditableString::view`] method.
#[derive(Debug)]
pub struct View<'a> {
    contents: &'a [char],
    cursor: usize,
}

impl Display for View<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for ch in self.contents {
            ch.fmt(f)?
        }
        Ok(())
    }
}

impl View<'_> {
    /// The position of the cursor within the view.
    pub fn position(&self) -> usize {
        self.cursor
    }
}

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
#[derive(Debug)]
pub struct EditableString {
    /// The contents of the editable string.
    contents: Vec<char>,
    /// The position within the string.
    cursor: usize,
    /// The current offset of the view window.
    offset: usize,
    /// The width of the view window.
    width: usize,
    /// Scratch space for operations such as non-append paste.
    _scratch: Vec<char>,
}

impl EditableString {
    pub fn full_contents(&self) -> String {
        self.contents.iter().collect()
    }

    pub fn view(&self) -> View<'_> {
        View {
            contents: &self.contents
                [self.offset..min(self.offset + self.width, self.contents.len())],
            cursor: self.cursor - self.offset,
        }
    }

    pub fn new(width: usize) -> Self {
        Self {
            contents: Vec::default(),
            cursor: 0,
            offset: 0,
            width,
            _scratch: Vec::new(),
        }
    }

    pub fn resize(&mut self, max_width: usize) {
        self.width = max_width;
        self.reset_window();
    }

    /// Is the cursor at the end of the string?
    pub fn cursor_at_end(&self) -> bool {
        self.cursor == self.contents.len()
    }

    /// Reset the window so that it includes the new cursor position `pos`
    #[inline]
    fn reset_window(&mut self) {
        if self.cursor < self.offset {
            self.offset = self.cursor
        } else if self.cursor >= self.offset + self.width {
            self.offset = self.cursor - self.width
        }
    }

    /// Change the cursor position to the provided index; return true if the cursor moved, else
    /// false.
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
        let changed = match st {
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
                self.shift_to(self.cursor + 1);
                true
            }
            Edit::Paste(s) => {
                if !s.is_empty() {
                    // we are appending, so we can avoid some writes.
                    if self.cursor_at_end() {
                        self.contents.extend(s.chars());
                        self.cursor = self.contents.len();
                    } else {
                        // cache tail inside scratch space
                        self._scratch.clear();
                        self._scratch.extend(self.contents.drain(self.cursor..));

                        // increment cursor and append the extension
                        self.shift_to(self.cursor + s.len());
                        self.contents.extend(s.chars());

                        // re-append the tail
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
        };

        if changed {
            self.reset_window();
        }

        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit() {
        let mut editable = EditableString::new(usize::MAX);
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

        assert_eq!(&editable.view().to_string(), "4abc123");
    }

    #[test]
    fn window() {
        let mut editable = EditableString::new(2);

        for e in [
            Edit::Insert('1'),
            Edit::Insert('2'),
            Edit::Insert('3'),
            Edit::Insert('4'),
            Edit::MoveLeft,
        ] {
            editable.edit(e);
        }
        assert_eq!(editable.view().position(), 1);

        editable.edit(Edit::MoveLeft);
        editable.edit(Edit::MoveLeft);
        assert_eq!(editable.view().position(), 0);

        editable.edit(Edit::Insert('a'));
        editable.edit(Edit::MoveToEnd);
        assert_eq!(editable.view().position(), 2);
    }
}
