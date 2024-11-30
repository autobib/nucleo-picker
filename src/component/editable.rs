use super::{Cursor, View};

/// Mutate a given string in-place, removing ASCII control characters and converting newlines,
/// carriage returns, and TABs to ASCII space.
pub fn normalize_query_string(s: &mut String) {
    *s = s.chars().filter_map(normalize_char).collect();
}

fn normalize_char(ch: char) -> Option<char> {
    match ch {
        '\n' | '\t' => Some(' '),
        ch if ch.is_ascii_control() => None,
        ch => Some(ch),
    }
}

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

/// An editable string type with a cursor and scrolling window.
///
/// The cursor indicates the current edit position and supported actions.
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct EditableString {
    /// The contents of the editable string.
    contents: Vec<char>,
    /// The position within the string.
    cursor: Cursor,
    /// Scratch space for operations such as non-append paste.
    scratch: Vec<char>,
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
            scratch: Vec::new(),
        }
    }

    /// Resize the window with the updated width.
    pub fn resize(&mut self, width: usize) {
        self.cursor.set_width(width);
    }

    /// Check if there is no trailing escape `\`.
    fn no_trailing_escape(&self) -> bool {
        (self
            .contents
            .iter()
            .rev()
            .take_while(|ch| **ch == '\\')
            .count()
            % 2)
            == 0
    }

    /// Are we in an "appending" state? This is the case if the cursor is at the end of the string
    /// and the previous character isn't an escaped `\`.
    pub fn is_appending(&self) -> bool {
        self.cursor.index() == self.contents.len() && self.no_trailing_escape()
    }

    /// Whether or not the query string is empty.
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Change the cursor position to the provided index; return true if the cursor moved, else
    /// false.
    #[inline]
    fn shift_to(&mut self, pos: usize) -> bool {
        if pos <= self.contents.len() && self.cursor.index() != pos {
            self.cursor.set_index(pos);
            true
        } else {
            false
        }
    }

    /// Move the cursor by the provided [`Jump`].
    #[inline]
    #[allow(clippy::needless_pass_by_value)]
    fn jump(&mut self, jm: Jump) -> bool {
        match jm {
            Jump::Left(dist) => self.shift_to(self.cursor.index().saturating_sub(dist)),
            Jump::Right(dist) => self.shift_to(self.cursor.index().saturating_add(dist)),
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
                if let Some(ch) = normalize_char(ch) {
                    self.contents.insert(self.cursor.index(), ch);
                    self.jump(Jump::Right(1))
                } else {
                    false
                }
            }
            Edit::Paste(mut s) => {
                if s.is_empty() {
                    false
                } else {
                    normalize_query_string(&mut s);
                    // we are appending, so we can avoid some writes.
                    if self.is_appending() {
                        self.contents.extend(s.chars());
                        self.jump(Jump::ToEnd);
                    } else {
                        // move tail to scratch space
                        self.scratch.clear();
                        self.scratch
                            .extend(self.contents.drain(self.cursor.index()..));

                        // increment cursor and append the extension
                        self.contents.extend(s.chars());
                        self.jump(Jump::Right(s.len()));

                        // re-append the tail
                        self.contents.append(&mut self.scratch);
                    }
                    true
                }
            }
            Edit::Delete => {
                let changed = self.jump(Jump::Left(1));
                if changed {
                    self.contents.remove(self.cursor.index());
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
    fn test_normalize_query() {
        let mut s = "a\nb".to_owned();
        normalize_query_string(&mut s);
        assert_eq!(s, "a b");

        let mut s = "ｏ\nｏ".to_owned();
        normalize_query_string(&mut s);
        assert_eq!(s, "ｏ ｏ");

        let mut s = "a\n\u{07}ｏ".to_owned();
        normalize_query_string(&mut s);
        assert_eq!(s, "a ｏ");
    }

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
