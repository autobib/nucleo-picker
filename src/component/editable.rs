use super::{Cursor, View};

/// An edit to apply to an [`EditableString`].
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    /// Insert a [`char`] at the current cursor position.
    Insert(char),
    /// Delete a [`char`] immediately preceding the current cursor position.
    Delete,
    /// Paste a [`String`] at the current cursor position.
    Paste(String),
    /// Move the cursor left.
    Left,
    /// Move the cursor right.
    Right,
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
}

/// A movement to apply to an [`EditableString`].
#[derive(Debug, PartialEq, Eq)]
enum Jump {
    /// Move the cursor left by a given number of characters.
    Left(usize),
    /// Move the cursor right by a given number of characters.
    Right(usize),
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
}

/// # An editable string type with a cursor and scrolling window.
/// This is an editable string type with a cursor indicating the current edit position, and various
/// supported actions.
#[derive(Debug)]
pub struct EditableString {
    /// The contents of the editable string.
    contents: Vec<char>,
    /// The position within the string.
    cursor: Cursor,
    /// Scratch space for operations such as non-append paste.
    _scratch: Vec<char>,
}

impl EditableString {
    pub fn full_contents(&self) -> String {
        self.contents.iter().collect()
    }

    /// Return an unpadded view; equivalent to
    /// [`EditableString::view_padded(0,0)`](EditableString::view_padded).
    #[allow(unused)]
    #[inline]
    pub fn view(&self) -> View<'_, char> {
        self.cursor.view(&self.contents)
    }

    /// Reset the prompt contents and move the cursor to the end of the prompt.
    pub fn set_prompt(&mut self, new: &str) {
        self.contents = new.chars().collect();
        self.jump(Jump::ToEnd);
    }

    /// Return the padded view given the current cursor position with padding size on the left
    /// and the right
    pub fn view_padded(&self, left: usize, right: usize) -> View<'_, char> {
        self.cursor.view_padded(left, right, &self.contents)
    }

    /// Create a new [`EditableString`] with given window width.
    pub fn new(width: usize) -> Self {
        Self {
            contents: Vec::default(),
            cursor: Cursor::new(width),
            _scratch: Vec::new(),
        }
    }

    /// Resize the window with the updated width.
    pub fn resize(&mut self, width: usize) {
        self.cursor.set_width(width);
    }

    /// Is the cursor at the end of the string?
    pub fn cursor_at_end(&self) -> bool {
        self.cursor.idx() == self.contents.len()
    }

    /// Change the cursor position to the provided index; return true if the cursor moved, else
    /// false.
    #[inline(always)]
    fn shift_to(&mut self, pos: usize) -> bool {
        if pos <= self.contents.len() && self.cursor.idx() != pos {
            self.cursor.set_index(pos);
            true
        } else {
            false
        }
    }

    /// Move the cursor by the provided [`Jump`].
    #[inline(always)]
    fn jump(&mut self, jm: Jump) -> bool {
        match jm {
            Jump::Left(dist) => self.shift_to(self.cursor.idx().saturating_sub(dist)),
            Jump::Right(dist) => self.shift_to(self.cursor.idx().saturating_add(dist)),
            Jump::ToStart => self.shift_to(0),
            Jump::ToEnd => self.shift_to(self.contents.len()),
        }
    }

    /// Apply the provided [`Edit`] to the [`EditableString`], and return whether or not anything
    /// changed.
    pub fn edit(&mut self, e: Edit) -> bool {
        match e {
            Edit::Left => self.jump(Jump::Left(1)),
            Edit::Right => self.jump(Jump::Right(1)),
            Edit::ToStart => self.jump(Jump::ToStart),
            Edit::ToEnd => self.jump(Jump::ToEnd),
            Edit::Insert(ch) => {
                self.contents.insert(self.cursor.idx(), ch);
                self.jump(Jump::Right(1))
            }
            Edit::Paste(s) => {
                if !s.is_empty() {
                    // we are appending, so we can avoid some writes.
                    if self.cursor_at_end() {
                        self.contents.extend(s.chars());
                        self.jump(Jump::ToEnd);
                    } else {
                        // move tail to scratch space
                        self._scratch.clear();
                        self._scratch
                            .extend(self.contents.drain(self.cursor.idx()..));

                        // increment cursor and append the extension
                        self.contents.extend(s.chars());
                        self.jump(Jump::Right(s.len()));

                        // re-append the tail
                        self.contents.append(&mut self._scratch);
                    }
                    true
                } else {
                    false
                }
            }
            Edit::Delete => {
                let changed = self.jump(Jump::Left(1));
                if changed {
                    self.contents.remove(self.cursor.idx());
                }
                changed
            }
        }
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
            Edit::Left,
            Edit::Insert('1'),
            Edit::Insert('2'),
            Edit::Insert('3'),
            Edit::ToStart,
            Edit::Delete,
            Edit::Insert('4'),
            Edit::ToEnd,
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
            Edit::Left,
        ] {
            editable.edit(e);
        }
        assert_eq!(editable.view().index(), 1);

        editable.edit(Edit::Left);
        editable.edit(Edit::Left);
        assert_eq!(editable.view().index(), 0);

        editable.edit(Edit::Insert('a'));
        editable.edit(Edit::ToEnd);
        assert_eq!(editable.view().index(), 2);
    }
}
