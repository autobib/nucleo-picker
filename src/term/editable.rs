use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Mutate a given string in-place, removing ASCII control characters and converting newlines,
/// carriage returns, and TABs to ASCII space.
pub fn normalize_query_string(s: &mut String) {
    *s = s
        .chars()
        .filter_map(normalize_char)
        .map(|(ch, _)| ch)
        .collect();
}

/// Normalize a single char, returning the resulting char as well as the width.
///
/// This automaticlly removes control characters since `ch.width()` returns `None` for control
/// characters.
#[inline]
fn normalize_char(ch: char) -> Option<(char, usize)> {
    match ch {
        '\n' | '\t' => Some((' ', 1)),
        ch => ch.width().map(|w| (ch, w)),
    }
}

/// An edit to apply to an [`EditableString`].
#[derive(Debug, PartialEq, Eq)]
pub enum Edit {
    /// Insert a [`char`] at the current cursor position.
    Insert(char),
    /// Delete a grapheme immediately preceding the current cursor position.
    Backspace,
    /// Delete the word immediately preceding the current cursor position.
    BackspaceWord,
    /// Delete a grapheme immediately following the current cursor position.
    Delete,
    /// Paste a [`String`] at the current cursor position.
    Paste(String),
    /// Move the cursor left.
    Left,
    /// Move the cursor left an entire word.
    WordLeft,
    /// Move the cursor right.
    Right,
    /// Move the cursor right an entire word.
    WordRight,
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
    /// Delete everything before the cursor.
    ClearBefore,
    /// Delete everything after the cursor.
    ClearAfter,
}

/// A movement to apply to an [`EditableString`].
#[derive(Debug, PartialEq, Eq)]
enum CursorMovement {
    /// Move the cursor left.
    Left,
    /// Move the cursor left an entire word.
    WordLeft,
    /// Move the cursor right.
    Right,
    /// Move the cursor right an entire word.
    WordRight,
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
}

#[derive(Debug)]
pub struct EditableString {
    contents: String,
    offset: usize,
    screen_offset: u16,
    width: u16,
    left_padding: u16,
    right_padding: u16,
}

impl EditableString {
    /// Create a new editable string with the provided screen width and padding.
    pub fn new(width: u16, padding: u16) -> Self {
        let prompt_padding = padding.min(width.saturating_sub(1) / 2);
        Self {
            contents: String::new(),
            offset: 0,
            screen_offset: 0,
            width,
            left_padding: prompt_padding,
            right_padding: prompt_padding,
        }
    }

    /// Whether or not the prompt is empty.
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Return the prompt contents as well as an 'offset' which is required in the presence of an
    /// initial grapheme that is too large to fit at the beginning of the screen.
    pub fn view(&self) -> (&str, u16) {
        let mut left_indices = self.contents[..self.offset].grapheme_indices(true).rev();
        let mut total_left_width = 0;
        let (left_offset, extra) = loop {
            match left_indices.next() {
                Some((offset, grapheme)) => {
                    total_left_width += grapheme.width();
                    if total_left_width >= self.screen_offset.into() {
                        let extra = (total_left_width - self.screen_offset as usize) as u16;
                        break (
                            offset
                                + if total_left_width == self.screen_offset.into() {
                                    0
                                } else {
                                    grapheme.len()
                                },
                            extra,
                        );
                    }
                }
                None => break (0, 0),
            }
        };

        let mut right_indices = self.contents[self.offset..].grapheme_indices(true);
        let mut total_right_width = 0;
        let max_right_width = self.width - self.screen_offset;
        let right_offset = loop {
            match right_indices.next() {
                Some((offset, grapheme)) => {
                    total_right_width += grapheme.width();
                    if total_right_width > max_right_width as usize {
                        break self.offset + offset;
                    }
                }
                None => break self.contents.len(),
            }
        };

        (&self.contents[left_offset..right_offset], extra)
    }

    /// Resize the screen, adjusting the padding and the screen width.
    pub fn resize(&mut self, width: u16, padding: u16) {
        let prompt_padding = padding.min(width.saturating_sub(1) / 2);
        self.left_padding = prompt_padding;
        self.right_padding = prompt_padding;
        self.width = width;
        self.screen_offset = self.screen_offset.min(width - prompt_padding);
    }

    /// Get the cursor offset within the screen.
    pub fn screen_offset(&self) -> u16 {
        self.screen_offset
    }

    /// Get the contents of the prompt.
    pub fn contents(&self) -> &str {
        &self.contents
    }

    /// Reset the prompt, moving the cursor to the end.
    pub fn set_prompt<Q: Into<String>>(&mut self, prompt: Q) {
        self.contents = prompt.into();
        self.offset = self.contents.len();
    }

    /// Increase the screen offset by the provided width, without exceeding the maximum offset.
    fn increase_by_width(&mut self, width: usize) {
        self.screen_offset = self
            .screen_offset
            .saturating_add(width.try_into().unwrap_or(u16::MAX))
            .min(self.width - self.right_padding);
    }

    /// Insert a character at the cursor position.
    fn insert_char(&mut self, ch: char, w: usize) -> bool {
        self.contents.insert(self.offset, ch);
        self.increase_by_width(w);
        self.offset += ch.len_utf8();
        true
    }

    /// Insert a string at the cursor position.
    fn insert(&mut self, string: &str) -> bool {
        self.contents.insert_str(self.offset, string);
        self.increase_by_width(string.width());
        self.offset += string.len();
        true
    }

    #[inline]
    fn move_left(&self, width: usize) -> u16 {
        // check if we would hit the beginning of the string
        let mut total_left_width = 0;
        let mut graphemes = self.contents[..self.offset].graphemes(true).rev();
        let left_padding = loop {
            match graphemes.next() {
                Some(g) => {
                    total_left_width += g.width();
                    if total_left_width >= self.left_padding as usize {
                        break self.left_padding;
                    }
                }
                None => {
                    break total_left_width as u16;
                }
            }
        };

        self.screen_offset
            .saturating_sub(width.try_into().unwrap_or(u16::MAX))
            .max(left_padding)
    }

    /// Move the cursor.
    #[inline]
    #[allow(clippy::needless_pass_by_value)]
    fn move_cursor(&mut self, cm: CursorMovement) -> bool {
        match cm {
            CursorMovement::Left => {
                match self.contents[..self.offset]
                    .grapheme_indices(true)
                    .next_back()
                {
                    Some((new_offset, gp)) => {
                        self.offset = new_offset;
                        self.screen_offset = self.move_left(gp.width());
                        true
                    }
                    None => false,
                }
            }
            CursorMovement::WordLeft => {
                match self.contents[..self.offset]
                    .unicode_word_indices()
                    .next_back()
                {
                    Some((new_offset, _)) => {
                        let step_width = self.contents[new_offset..self.offset].width();
                        self.offset = new_offset;
                        self.screen_offset = self.move_left(step_width);
                        true
                    }
                    None => false,
                }
            }
            CursorMovement::Right => match self.contents[self.offset..].graphemes(true).next() {
                Some(gp) => {
                    self.offset += gp.len();
                    self.increase_by_width(gp.width());
                    true
                }
                None => false,
            },
            CursorMovement::WordRight => {
                let mut word_indices = self.contents[self.offset..].unicode_word_indices();
                if word_indices.next().is_some() {
                    let next_offset = word_indices
                        .next()
                        .map(|(s, _)| self.offset + s)
                        .unwrap_or(self.contents.len());
                    let step_width = self.contents[self.offset..next_offset].width();
                    self.offset = next_offset;
                    self.increase_by_width(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::ToStart => {
                if self.offset == 0 {
                    false
                } else {
                    self.offset = 0;
                    self.screen_offset = 0;
                    true
                }
            }
            CursorMovement::ToEnd => {
                if self.offset == self.contents.len() {
                    false
                } else {
                    let max_offset = self.width - self.right_padding;
                    for gp in self.contents[self.offset..].graphemes(true) {
                        self.screen_offset = self
                            .screen_offset
                            .saturating_add(gp.width().try_into().unwrap_or(u16::MAX));
                        if self.screen_offset >= max_offset {
                            self.screen_offset = max_offset;
                            break;
                        }
                    }
                    self.offset = self.contents.len();
                    true
                }
            }
        }
    }

    /// Edit the editable string according to the provided [`Edit`] action.
    pub fn edit(&mut self, e: Edit) -> bool {
        match e {
            Edit::Left => self.move_cursor(CursorMovement::Left),
            Edit::WordLeft => self.move_cursor(CursorMovement::WordLeft),
            Edit::Right => self.move_cursor(CursorMovement::Right),
            Edit::WordRight => self.move_cursor(CursorMovement::WordRight),
            Edit::ToStart => self.move_cursor(CursorMovement::ToStart),
            Edit::ToEnd => self.move_cursor(CursorMovement::ToEnd),
            Edit::Insert(ch) => {
                if let Some((ch, w)) = normalize_char(ch) {
                    self.insert_char(ch, w)
                } else {
                    false
                }
            }
            Edit::Paste(mut s) => {
                normalize_query_string(&mut s);
                self.insert(&s)
            }
            Edit::Backspace => {
                let delete_until = self.offset;
                if self.move_cursor(CursorMovement::Left) {
                    self.contents.replace_range(self.offset..delete_until, "");
                    true
                } else {
                    false
                }
            }
            Edit::BackspaceWord => {
                let delete_until = self.offset;
                if self.move_cursor(CursorMovement::WordLeft) {
                    self.contents.replace_range(self.offset..delete_until, "");
                    true
                } else {
                    false
                }
            }
            Edit::ClearBefore => {
                if self.offset == 0 {
                    false
                } else {
                    self.contents.replace_range(..self.offset, "");
                    self.offset = 0;
                    self.screen_offset = 0;
                    true
                }
            }
            Edit::Delete => match self.contents[self.offset..].graphemes(true).next() {
                Some(next) => {
                    self.contents
                        .replace_range(self.offset..self.offset + next.len(), "");
                    true
                }
                None => false,
            },
            Edit::ClearAfter => {
                if self.offset == self.contents.len() {
                    false
                } else {
                    self.contents.truncate(self.offset);
                    true
                }
            }
        }
    }

    /// Check if there is no trailing escape `\`.
    fn no_trailing_escape(&self) -> bool {
        (self
            .contents
            .bytes()
            .rev()
            .take_while(|ch| *ch == b'\\')
            .count()
            % 2)
            == 0
    }

    /// Are we in an "appending" state? This is the case if the cursor is at the end of the string
    /// and the previous character isn't an escaped `\`.
    pub fn is_appending(&self) -> bool {
        self.offset == self.contents.len() && self.no_trailing_escape()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout() {
        let mut editable = EditableString::new(6, 2);
        editable.edit(Edit::Insert('a'));
        assert_eq!(editable.screen_offset, 1);
        editable.edit(Edit::Insert('Ａ'));
        assert_eq!(editable.screen_offset, 3);
        editable.edit(Edit::Insert('B'));
        assert_eq!(editable.screen_offset, 4);

        let mut editable = EditableString::new(6, 2);
        editable.edit(Edit::Paste("ＡaＡ".to_owned()));
        assert_eq!(editable.screen_offset, 4);

        let mut editable = EditableString::new(6, 2);
        editable.edit(Edit::Paste("abc".to_owned()));
        assert_eq!(editable.screen_offset, 3);
        editable.edit(Edit::Paste("ab".to_owned()));
        assert_eq!(editable.screen_offset, 4);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 3);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 1);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 0);

        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("ＡＡＡＡＡ".to_owned()));
        editable.edit(Edit::ToStart);
        assert_eq!(editable.screen_offset, 0);
        editable.edit(Edit::Right);
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Right);
        assert_eq!(editable.screen_offset, 4);
        editable.edit(Edit::Right);
        assert_eq!(editable.screen_offset, 5);
        editable.edit(Edit::Right);
        assert_eq!(editable.screen_offset, 5);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 3);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Left);
        assert_eq!(editable.screen_offset, 0);

        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("abc".to_owned()));
        editable.edit(Edit::ToStart);
        editable.edit(Edit::ToEnd);
        assert_eq!(editable.screen_offset, 3);
        editable.edit(Edit::Paste("defghi".to_owned()));
        editable.edit(Edit::ToStart);
        editable.edit(Edit::ToEnd);
        assert_eq!(editable.screen_offset, 5);
    }

    #[test]
    fn test_view() {
        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("abc".to_owned()));
        assert_eq!(editable.view(), ("abc", 0));

        let mut editable = EditableString::new(6, 1);
        editable.edit(Edit::Paste("ＡＡＡＡＡＡ".to_owned()));
        assert_eq!(editable.view(), ("ＡＡ", 1));

        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("ＡＡＡＡ".to_owned()));
        assert_eq!(editable.view(), ("ＡＡ", 1));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("ＡＡ", 1));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("ＡＡＡ", 0));

        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("012345678".to_owned()));
        editable.edit(Edit::ToStart);
        assert_eq!(editable.view(), ("0123456", 0));

        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("012345Ａ".to_owned()));
        editable.edit(Edit::ToStart);
        assert_eq!(editable.view(), ("012345", 0));

        let mut editable = EditableString::new(4, 1);
        editable.edit(Edit::Paste("01234567".to_owned()));
        assert_eq!(editable.view(), ("567", 0));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("567", 0));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("567", 0));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("4567", 0));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("3456", 0));
        editable.edit(Edit::Left);
        assert_eq!(editable.view(), ("2345", 0));
        editable.edit(Edit::Right);
        assert_eq!(editable.view(), ("2345", 0));
        editable.edit(Edit::Right);
        assert_eq!(editable.view(), ("2345", 0));
        editable.edit(Edit::Right);
        assert_eq!(editable.view(), ("3456", 0));
    }

    #[test]
    fn test_word_movement() {
        let mut editable = EditableString::new(100, 2);
        editable.edit(Edit::Paste("one two".to_owned()));
        editable.edit(Edit::WordLeft);
        editable.edit(Edit::WordLeft);
        assert_eq!(editable.screen_offset, 0);
        editable.edit(Edit::WordRight);
        assert_eq!(editable.screen_offset, 4);
        editable.edit(Edit::WordRight);
        assert_eq!(editable.screen_offset, 7);
        editable.edit(Edit::WordRight);
        assert_eq!(editable.screen_offset, 7);
    }

    #[test]
    fn test_clear() {
        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("Ａbcde".to_owned()));
        editable.edit(Edit::ToStart);
        editable.edit(Edit::Right);
        editable.edit(Edit::Right);
        editable.edit(Edit::ClearAfter);
        assert_eq!(editable.contents, "Ａb");
        editable.edit(Edit::Insert('c'));
        editable.edit(Edit::Left);
        editable.edit(Edit::ClearBefore);
        assert_eq!(editable.contents, "c");
    }

    #[test]
    fn test_delete() {
        let mut editable = EditableString::new(7, 2);
        editable.edit(Edit::Paste("Ａb".to_owned()));
        editable.edit(Edit::Backspace);
        assert_eq!(editable.contents, "Ａ");
        assert_eq!(editable.screen_offset, 2);
        editable.edit(Edit::Backspace);
        assert_eq!(editable.contents, "");
        assert_eq!(editable.screen_offset, 0);
    }

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
    fn test_editable() {
        let mut editable = EditableString::new(3, 1);
        for e in [
            Edit::Insert('a'),
            Edit::Left,
            Edit::Insert('b'),
            Edit::ToEnd,
            Edit::Insert('c'),
            Edit::ToStart,
            Edit::Insert('d'),
            Edit::Left,
            Edit::Left,
            Edit::Right,
            Edit::Insert('e'),
        ] {
            editable.edit(e);
        }
        assert_eq!(editable.contents, "debac");

        let mut editable = EditableString::new(3, 1);
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
            Edit::Backspace,
            Edit::Insert('4'),
            Edit::ToEnd,
            Edit::Backspace,
            Edit::Left,
            Edit::Delete,
        ] {
            editable.edit(e);
        }

        assert_eq!(editable.contents, "4abc12");
    }

    #[test]
    fn test_editable_unicode() {
        let mut editable = EditableString::new(3, 1);
        for e in [
            Edit::Paste("दे".to_owned()),
            Edit::Left,
            Edit::Insert('a'),
            Edit::ToEnd,
            Edit::Insert('Ａ'),
        ] {
            editable.edit(e);
        }
        assert_eq!(editable.contents, "aदेＡ");

        for e in [
            Edit::ToStart,
            Edit::Right,
            Edit::ToEnd,
            Edit::Left,
            Edit::Backspace,
        ] {
            editable.edit(e);
        }

        assert_eq!(editable.contents, "aＡ");
    }
}
