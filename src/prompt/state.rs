use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::{PromptConfig, PromptEvent};
use crate::util::as_u16;

/// A cursor representing the current position in the prompt string `s`.
trait Cursor {
    fn right(self, s: &str, steps: usize) -> Self;

    fn right_word(self, s: &str, steps: usize) -> Self;

    fn left(self, s: &str, steps: usize) -> Self;

    fn left_word(self, s: &str, steps: usize) -> Self;
}

impl Cursor for usize {
    fn right(self, s: &str, steps: usize) -> Self {
        match s[self..].grapheme_indices(true).nth(steps) {
            Some((offset, _)) => self + offset,
            None => s.len(),
        }
    }

    fn right_word(self, s: &str, steps: usize) -> Self {
        match s[self..].unicode_word_indices().nth(steps) {
            Some((offset, _)) => self + offset,
            None => s.len(),
        }
    }

    fn left(self, s: &str, steps: usize) -> Self {
        match s[..self].grapheme_indices(true).rev().take(steps).last() {
            Some((offset, _)) => offset,
            None => 0,
        }
    }

    fn left_word(self, s: &str, steps: usize) -> Self {
        match s[..self].unicode_word_indices().rev().take(steps).last() {
            Some((offset, _)) => offset,
            None => 0,
        }
    }
}

/// Mutate a given string in-place, removing ASCII control characters and converting newlines,
/// carriage returns, and TABs to ASCII space.
pub(super) fn normalize_prompt_string(s: &mut String) {
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

/// A cursor movement applied to the [`PromptState`].
#[derive(Debug, PartialEq, Eq)]
enum CursorMovement {
    /// Move the cursor left.
    Left(usize),
    /// Move the cursor left an entire word.
    WordLeft(usize),
    /// Move the cursor right.
    Right(usize),
    /// Move the cursor right an entire word.
    WordRight(usize),
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
}

/// A wrapper type encapsulating all of the prompt.
#[derive(Debug)]
pub(crate) struct PromptState {
    pub(crate) data: PromptData,
    pub(crate) view: PromptView,
}

#[derive(Debug, Default)]
pub(crate) struct PromptData {
    contents: String,
    offset: usize,
}

impl PromptData {
    /// Get the contents of the prompt.
    pub fn contents(&self) -> &str {
        &self.contents
    }
}

#[derive(Debug)]
pub(crate) struct PromptView {
    screen_offset: u16,
    pub(super) width: u16,
    config: PromptConfig,
}

impl PromptState {
    /// Create a new editable string with initial screen width and maximum padding.
    pub fn new(config: PromptConfig) -> Self {
        Self {
            data: PromptData::default(),
            view: PromptView {
                screen_offset: 0,
                width: 0,
                config,
            },
        }
    }

    pub fn set_query<Q: Into<String>>(&mut self, query: Q) -> bool {
        self.view.set_query(&mut self.data, query)
    }

    pub fn contents(&self) -> &str {
        self.data.contents()
    }

    pub fn handle(&mut self, event: PromptEvent) -> PromptStatus {
        self.view.handle(&mut self.data, event)
    }

    pub fn resize(&mut self, width: u16) {
        self.view.resize(&self.data, width);
    }

    #[cfg(test)]
    pub fn view(&self) -> (&str, u16) {
        self.view.view(&self.data)
    }
}

impl PromptView {
    pub fn handle(&mut self, data: &mut PromptData, e: PromptEvent) -> PromptStatus {
        let mut contents_changed = false;

        let needs_redraw = match e {
            PromptEvent::Reset(s) => {
                contents_changed = self.set_query(data, s);
                true
            }
            PromptEvent::Left(n) => self.move_cursor(data, CursorMovement::Left(n)),
            PromptEvent::WordLeft(n) => self.move_cursor(data, CursorMovement::WordLeft(n)),
            PromptEvent::Right(n) => self.move_cursor(data, CursorMovement::Right(n)),
            PromptEvent::WordRight(n) => self.move_cursor(data, CursorMovement::WordRight(n)),
            PromptEvent::ToStart => self.move_cursor(data, CursorMovement::ToStart),
            PromptEvent::ToEnd => self.move_cursor(data, CursorMovement::ToEnd),
            PromptEvent::Insert(ch) => {
                if let Some((ch, _)) = normalize_char(ch) {
                    contents_changed = true;
                    self.insert_char(data, ch);
                    true
                } else {
                    false
                }
            }
            PromptEvent::Paste(mut s) => {
                normalize_prompt_string(&mut s);
                if !s.is_empty() {
                    contents_changed = true;
                    self.insert(data, &s);
                    true
                } else {
                    false
                }
            }
            PromptEvent::Backspace(n) => {
                let delete_until = data.offset;
                if self.move_cursor(data, CursorMovement::Left(n)) {
                    data.contents.replace_range(data.offset..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::BackspaceWord(n) => {
                let delete_until = data.offset;
                if self.move_cursor(data, CursorMovement::WordLeft(n)) {
                    data.contents.replace_range(data.offset..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearBefore => {
                if data.offset == 0 {
                    false
                } else {
                    data.contents.replace_range(..data.offset, "");
                    data.offset = 0;
                    self.screen_offset = 0;
                    contents_changed = true;
                    true
                }
            }
            PromptEvent::Delete(n) => {
                let new_offset = data.offset.right(&data.contents, n);
                if new_offset != data.offset {
                    data.contents.replace_range(data.offset..new_offset, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearAfter => {
                if data.offset == data.contents.len() {
                    false
                } else {
                    data.contents.truncate(data.offset);
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

    pub fn padding(&self) -> u16 {
        self.config.padding.min(self.width.saturating_sub(1) / 2)
    }

    /// Return the prompt contents as well as an 'offset' which is required in the presence of an
    /// initial grapheme that is too large to fit at the beginning of the screen.
    pub fn view<'a>(&self, data: &'a PromptData) -> (&'a str, u16) {
        if self.width == 0 {
            return ("", 0);
        }

        let mut left_indices = data.contents[..data.offset].grapheme_indices(true).rev();
        let mut total_left_width = 0;
        let (left_offset, extra) = loop {
            match left_indices.next() {
                Some((offset, grapheme)) => {
                    total_left_width += grapheme.width();
                    if total_left_width >= self.screen_offset.into() {
                        let extra = (total_left_width - self.screen_offset as usize) as u16;
                        break (
                            offset
                                + if total_left_width == usize::from(self.screen_offset) {
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

        let mut right_indices = data.contents[data.offset..].grapheme_indices(true);
        let mut total_right_width = 0;
        let max_right_width = self.width - self.screen_offset;
        let right_offset = loop {
            match right_indices.next() {
                Some((offset, grapheme)) => {
                    total_right_width += grapheme.width();
                    if total_right_width > max_right_width as usize {
                        break data.offset + offset;
                    }
                }
                None => break data.contents.len(),
            }
        };

        (&data.contents[left_offset..right_offset], extra)
    }

    /// Resize the screen, adjusting the padding and the screen width.
    pub fn resize(&mut self, data: &PromptData, width: u16) {
        self.width = width;

        let padding = self.padding();
        let capacity = width - padding;
        let before = as_u16(data.contents[..data.offset].width());
        let after = as_u16(data.contents[data.offset..].width());

        let upper = before.min(capacity);
        let lower = padding
            .min(before)
            .max(capacity.saturating_sub(after))
            .min(upper);

        self.screen_offset = self.screen_offset.clamp(lower, upper);
    }

    /// Get the cursor offset within the screen.
    pub fn screen_offset(&self) -> u16 {
        self.screen_offset
    }

    /// Reset the prompt, moving the cursor to the end.
    pub fn set_query<Q: Into<String>>(&mut self, data: &mut PromptData, prompt: Q) -> bool {
        let mut contents = prompt.into();
        normalize_prompt_string(&mut contents);
        let contents_changed = contents != data.contents;
        data.contents = contents;
        data.offset = data.contents.len();
        self.screen_offset = as_u16(data.contents.width()).min(self.width - self.padding());
        contents_changed
    }

    /// Increase the screen offset by the provided width, without exceeding the maximum offset.
    fn right_by(&mut self, width: usize) {
        self.screen_offset = self
            .screen_offset
            .saturating_add(as_u16(width))
            .min(self.width - self.padding());
    }

    /// Insert a character at the cursor position.
    fn insert_char(&mut self, data: &mut PromptData, ch: char) {
        let mut encoded = [0; 4];
        self.insert(data, ch.encode_utf8(&mut encoded));
    }

    /// Insert a string at the cursor position.
    fn insert(&mut self, data: &mut PromptData, string: &str) {
        let previous_grapheme = data.contents[..data.offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(data.offset, |(offset, _)| offset);
        let old_width = data.contents[previous_grapheme..data.offset].width();

        data.contents.insert_str(data.offset, string);
        data.offset += string.len();

        let new_width = data.contents[previous_grapheme..data.offset].width();
        if new_width > old_width {
            self.right_by(new_width - old_width);
        } else if old_width > new_width {
            self.left_by(data, old_width - new_width);
        }
    }

    #[inline]
    fn left_by(&mut self, data: &PromptData, width: usize) {
        // check if we would hit the beginning of the string
        let mut total_left_width = 0;
        let mut graphemes = data.contents[..data.offset].graphemes(true).rev();
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

        self.screen_offset = self
            .screen_offset
            .saturating_sub(as_u16(width))
            .max(left_padding);
    }

    /// Move the cursor.
    #[inline]
    #[allow(clippy::needless_pass_by_value)]
    fn move_cursor(&mut self, data: &mut PromptData, cm: CursorMovement) -> bool {
        match cm {
            CursorMovement::Left(n) => {
                let new_offset = data.offset.left(&data.contents, n);
                if new_offset != data.offset {
                    let step_width = data.contents[new_offset..data.offset].width();
                    data.offset = new_offset;
                    self.left_by(data, step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::WordLeft(n) => {
                let new_offset = data.offset.left_word(&data.contents, n);
                if new_offset != data.offset {
                    let step_width = data.contents[new_offset..data.offset].width();
                    data.offset = new_offset;
                    self.left_by(data, step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::Right(n) => {
                let new_offset = data.offset.right(&data.contents, n);
                if new_offset != data.offset {
                    let step_width = data.contents[data.offset..new_offset].width();
                    data.offset = new_offset;
                    self.right_by(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::WordRight(n) => {
                let new_offset = data.offset.right_word(&data.contents, n);
                if new_offset != data.offset {
                    let step_width = data.contents[data.offset..new_offset].width();
                    data.offset = new_offset;
                    self.right_by(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::ToStart => {
                if data.offset == 0 {
                    false
                } else {
                    data.offset = 0;
                    self.screen_offset = 0;
                    true
                }
            }
            CursorMovement::ToEnd => {
                if data.offset == data.contents.len() {
                    false
                } else {
                    let max_offset = self.width - self.padding();
                    for gp in data.contents[data.offset..].graphemes(true) {
                        self.screen_offset = self
                            .screen_offset
                            .saturating_add(gp.width().try_into().unwrap_or(u16::MAX));
                        if self.screen_offset >= max_offset {
                            self.screen_offset = max_offset;
                            break;
                        }
                    }
                    data.offset = data.contents.len();
                    true
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PromptStatus {
    pub needs_redraw: bool,
    pub contents_changed: bool,
}
