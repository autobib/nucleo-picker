use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{PromptConfig, PromptEvent};
use crate::util::as_u16;

/// Mutate a given string in-place, removing ASCII control characters and converting newlines,
/// carriage returns, and TABs to ASCII space.
pub(super) fn normalize_prompt_string(s: &mut String) {
    *s = s.chars().filter_map(normalize_char).collect();
}

/// Normalize a single character, or return None for unsupported control characters.
#[inline]
fn normalize_char(ch: char) -> Option<char> {
    match ch {
        '\n' | '\t' => Some(' '),
        // ch.width() returns `None` for control characters
        ch => ch.width().map(|_| ch),
    }
}

/// A struct holding persistent prompt text, cursor, and prompt viewport state.
#[derive(Debug)]
pub(crate) struct PromptState {
    contents: String,
    cursor_byte: usize,
    cursor_column: u16,
    width: u16,
    config: PromptConfig,
}

impl PromptState {
    /// Create a new editable string with initial screen width and maximum padding.
    pub fn new(config: PromptConfig) -> Self {
        Self {
            contents: String::new(),
            cursor_byte: 0,
            cursor_column: 0,
            width: 0,
            config,
        }
    }

    /// Get the contents of the prompt.
    pub fn contents(&self) -> &str {
        &self.contents
    }

    pub fn apply(&mut self, e: PromptEvent) -> PromptStatus {
        let mut contents_changed = false;

        let needs_redraw = match e {
            PromptEvent::Reset(s) => {
                contents_changed = self.set_query(s);
                true
            }
            PromptEvent::Left(n) => self.move_left_to(self.left_offset(n)),
            PromptEvent::WordLeft(n) => self.move_left_to(self.left_word_offset(n)),
            PromptEvent::Right(n) => self.move_right_to(self.right_offset(n)),
            PromptEvent::WordRight(n) => self.move_right_to(self.right_word_offset(n)),
            PromptEvent::ToStart => {
                if self.cursor_byte == 0 {
                    false
                } else {
                    self.cursor_byte = 0;
                    self.cursor_column = 0;
                    true
                }
            }
            PromptEvent::ToEnd => self.move_to_end(),
            PromptEvent::Insert(ch) => {
                if let Some(ch) = normalize_char(ch) {
                    contents_changed = true;
                    self.insert_char(ch);
                    true
                } else {
                    false
                }
            }
            PromptEvent::Paste(mut s) => {
                normalize_prompt_string(&mut s);
                if !s.is_empty() {
                    contents_changed = true;
                    self.insert(&s);
                    true
                } else {
                    false
                }
            }
            PromptEvent::Backspace(n) => {
                let delete_until = self.cursor_byte;
                if self.move_left_to(self.left_offset(n)) {
                    self.contents
                        .replace_range(self.cursor_byte..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::BackspaceWord(n) => {
                let delete_until = self.cursor_byte;
                if self.move_left_to(self.left_word_offset(n)) {
                    self.contents
                        .replace_range(self.cursor_byte..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearBefore => {
                if self.cursor_byte == 0 {
                    false
                } else {
                    self.contents.replace_range(..self.cursor_byte, "");
                    self.cursor_byte = 0;
                    self.cursor_column = 0;
                    contents_changed = true;
                    true
                }
            }
            PromptEvent::Delete(n) => {
                let new_offset = self.right_offset(n);
                if new_offset != self.cursor_byte {
                    self.contents
                        .replace_range(self.cursor_byte..new_offset, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearAfter => {
                if self.cursor_byte == self.contents.len() {
                    false
                } else {
                    self.contents.truncate(self.cursor_byte);
                    contents_changed = true;
                    true
                }
            }
        };

        PromptStatus {
            needs_redraw,
            contents_changed,
        }
    }

    fn padding(&self) -> u16 {
        self.config.padding.min(self.width.saturating_sub(1) / 2)
    }

    /// Return the prompt contents as well as an 'offset' which is required in the presence of an
    /// initial grapheme that is too large to fit at the beginning of the screen.
    pub fn view(&self) -> (&str, u16) {
        if self.width == 0 {
            return ("", 0);
        }

        let mut left_offset = self.cursor_byte;
        let mut extra = usize::from(self.cursor_column);
        for (offset, grapheme) in self.contents[..self.cursor_byte]
            .grapheme_indices(true)
            .rev()
        {
            let Some(remaining) = extra.checked_sub(grapheme.width()) else {
                break;
            };
            left_offset = offset;
            extra = remaining;
        }

        let mut right_indices = self.contents[self.cursor_byte..].grapheme_indices(true);
        let mut total_right_width = 0;
        let max_right_width = self.width - self.cursor_column;
        let right_offset = loop {
            match right_indices.next() {
                Some((offset, grapheme)) => {
                    total_right_width += grapheme.width();
                    if total_right_width > max_right_width as usize {
                        break self.cursor_byte + offset;
                    }
                }
                None => break self.contents.len(),
            }
        };

        (&self.contents[left_offset..right_offset], extra as u16)
    }

    /// Resize the screen, adjusting the padding and the screen width.
    pub fn resize(&mut self, width: u16) {
        if self.width == width {
            return;
        }
        self.width = width;

        let padding = self.padding();
        let capacity = width - padding;
        let before = as_u16(self.contents[..self.cursor_byte].width());
        let after = as_u16(self.contents[self.cursor_byte..].width());

        let upper = before.min(capacity);
        let lower = padding
            .min(before)
            .max(capacity.saturating_sub(after))
            .min(upper);

        self.cursor_column = self.cursor_column.clamp(lower, upper);
    }

    /// Get the cursor offset within the screen.
    pub fn cursor_column(&self) -> u16 {
        self.cursor_column
    }

    /// Reset the prompt, moving the cursor to the end.
    pub fn set_query<Q: Into<String>>(&mut self, prompt: Q) -> bool {
        let mut contents = prompt.into();
        normalize_prompt_string(&mut contents);
        let contents_changed = contents != self.contents;
        self.contents = contents;
        self.cursor_byte = self.contents.len();
        self.cursor_column = as_u16(self.contents.width()).min(self.width - self.padding());
        contents_changed
    }

    /// Increase the screen offset by the provided width, without exceeding the maximum offset.
    fn right_by(&mut self, width: usize) {
        self.cursor_column = self
            .cursor_column
            .saturating_add(as_u16(width))
            .min(self.width - self.padding());
    }

    /// Insert a character at the cursor position.
    fn insert_char(&mut self, ch: char) {
        let mut encoded = [0; 4];
        self.insert(ch.encode_utf8(&mut encoded));
    }

    /// Insert a string at the cursor position.
    fn insert(&mut self, string: &str) {
        let previous_grapheme = self.contents[..self.cursor_byte]
            .grapheme_indices(true)
            .next_back()
            .map_or(self.cursor_byte, |(offset, _)| offset);
        let old_width = self.contents[previous_grapheme..self.cursor_byte].width();

        self.contents.insert_str(self.cursor_byte, string);
        self.cursor_byte += string.len();

        let new_width = self.contents[previous_grapheme..self.cursor_byte].width();
        if new_width > old_width {
            self.right_by(new_width - old_width);
        } else if old_width > new_width {
            self.left_by(old_width - new_width);
        }
    }

    #[inline]
    fn left_by(&mut self, width: usize) {
        // check if we would hit the beginning of the string
        let mut total_left_width = 0;
        let mut graphemes = self.contents[..self.cursor_byte].graphemes(true).rev();
        let left_padding = loop {
            match graphemes.next() {
                Some(g) => {
                    total_left_width += g.width();
                    let left_padding = self.padding();
                    if total_left_width >= left_padding as usize {
                        break left_padding;
                    }
                }
                None => {
                    break total_left_width as u16;
                }
            }
        };

        self.cursor_column = self
            .cursor_column
            .saturating_sub(as_u16(width))
            .max(left_padding);
    }

    fn left_offset(&self, steps: usize) -> usize {
        self.contents[..self.cursor_byte]
            .grapheme_indices(true)
            .rev()
            .take(steps)
            .fold(self.cursor_byte, |_, (offset, _)| offset)
    }

    fn left_word_offset(&self, steps: usize) -> usize {
        let mut words = self.contents[..self.cursor_byte]
            .unicode_word_indices()
            .rev();
        // the old starts at the cursor, but we need to start a zero byte at the beginning in
        // order to correctly handle the empty word iterator so that positively many
        // steps will consume whitespace/punctuation-only prefixes as well
        std::iter::once(words.next().unwrap_or((0, "")))
            .chain(words)
            .take(steps)
            .fold(self.cursor_byte, |_, (offset, _)| offset)
    }

    fn right_offset(&self, steps: usize) -> usize {
        self.contents[self.cursor_byte..]
            .grapheme_indices(true)
            .nth(steps)
            .map_or(self.contents.len(), |(offset, _)| self.cursor_byte + offset)
    }

    fn right_word_offset(&self, steps: usize) -> usize {
        self.contents[self.cursor_byte..]
            .unicode_word_indices()
            .nth(steps)
            .map_or(self.contents.len(), |(offset, _)| self.cursor_byte + offset)
    }

    fn move_left_to(&mut self, new_offset: usize) -> bool {
        if new_offset == self.cursor_byte {
            return false;
        }
        let step_width = self.contents[new_offset..self.cursor_byte].width();
        self.cursor_byte = new_offset;
        self.left_by(step_width);
        true
    }

    fn move_right_to(&mut self, new_offset: usize) -> bool {
        if new_offset == self.cursor_byte {
            return false;
        }
        let step_width = self.contents[self.cursor_byte..new_offset].width();
        self.cursor_byte = new_offset;
        self.right_by(step_width);
        true
    }

    fn move_to_end(&mut self) -> bool {
        if self.cursor_byte == self.contents.len() {
            return false;
        }
        let max_offset = self.width - self.padding();
        for gp in self.contents[self.cursor_byte..].graphemes(true) {
            self.cursor_column = self.cursor_column.saturating_add(as_u16(gp.width()));
            if self.cursor_column >= max_offset {
                self.cursor_column = max_offset;
                break;
            }
        }
        self.cursor_byte = self.contents.len();
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PromptStatus {
    pub needs_redraw: bool,
    pub contents_changed: bool,
}
