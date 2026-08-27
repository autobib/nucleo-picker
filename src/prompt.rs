#[cfg(test)]
mod tests;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{component::ComponentStatus, util::as_u16};

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
pub fn normalize_prompt_string(s: &mut String) {
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

/// An event that modifies the prompt.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PromptEvent {
    /// Move the cursor `usize` graphemes to the left.
    Left(usize),
    /// Move the cursor `usize` Unicode words to the left.
    WordLeft(usize),
    /// Move the cursor `usize` graphemes to the right.
    Right(usize),
    /// Move the cursor `usize` Unicode words to the right.
    WordRight(usize),
    /// Move the cursor to the start.
    ToStart,
    /// Move the cursor to the end.
    ToEnd,
    /// Delete `usize` graphemes immediately preceding the cursor.
    Backspace(usize),
    /// Delete `usize` graphemes immediately following the cursor.
    Delete(usize),
    /// Delete `usize` Unicode words immediately preceding the cursor.
    BackspaceWord(usize),
    /// Clear everything before the cursor.
    ClearBefore,
    /// Clear everything after the cursor.
    ClearAfter,
    /// Insert a character at the cursor.
    Insert(char),
    /// Paste a string at the cursor.
    Paste(String),
    /// Reset the prompt to a new string and move the cursor to the end.
    Reset(String),
}

impl PromptEvent {
    /// Whether or not the event is a cursor movement that does not edit the prompt string.
    #[must_use]
    pub fn is_cursor_movement(&self) -> bool {
        matches!(
            &self,
            Self::Left(_)
                | Self::WordLeft(_)
                | Self::Right(_)
                | Self::WordRight(_)
                | Self::ToStart
                | Self::ToEnd
        )
    }
}

/// A movement to apply to an [`Prompt`].
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

#[derive(Debug, Clone)]
pub struct PromptConfig {
    pub padding: u16,
}

impl PromptConfig {
    pub const fn new() -> Self {
        Self { padding: 2 }
    }
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub struct Prompt {
    contents: String,
    offset: usize,
    screen_offset: u16,
    width: u16,
    config: PromptConfig,
}

impl Prompt {
    /// Create a new editable string with initial screen width and maximum padding.
    pub fn new(config: PromptConfig) -> Self {
        Self {
            contents: String::new(),
            offset: 0,
            screen_offset: 0,
            width: 0,
            config,
        }
    }

    pub fn handle(&mut self, e: PromptEvent) -> PromptStatus {
        let mut contents_changed = false;

        let needs_redraw = match e {
            PromptEvent::Reset(s) => {
                contents_changed = self.set_query(s);
                true
            }
            PromptEvent::Left(n) => self.move_cursor(CursorMovement::Left(n)),
            PromptEvent::WordLeft(n) => self.move_cursor(CursorMovement::WordLeft(n)),
            PromptEvent::Right(n) => self.move_cursor(CursorMovement::Right(n)),
            PromptEvent::WordRight(n) => self.move_cursor(CursorMovement::WordRight(n)),
            PromptEvent::ToStart => self.move_cursor(CursorMovement::ToStart),
            PromptEvent::ToEnd => self.move_cursor(CursorMovement::ToEnd),
            PromptEvent::Insert(ch) => {
                if let Some((ch, _)) = normalize_char(ch) {
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
                let delete_until = self.offset;
                if self.move_cursor(CursorMovement::Left(n)) {
                    self.contents.replace_range(self.offset..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::BackspaceWord(n) => {
                let delete_until = self.offset;
                if self.move_cursor(CursorMovement::WordLeft(n)) {
                    self.contents.replace_range(self.offset..delete_until, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearBefore => {
                if self.offset == 0 {
                    false
                } else {
                    self.contents.replace_range(..self.offset, "");
                    self.offset = 0;
                    self.screen_offset = 0;
                    contents_changed = true;
                    true
                }
            }
            PromptEvent::Delete(n) => {
                let new_offset = self.offset.right(&self.contents, n);
                if new_offset != self.offset {
                    self.contents.replace_range(self.offset..new_offset, "");
                    contents_changed = true;
                    true
                } else {
                    false
                }
            }
            PromptEvent::ClearAfter => {
                if self.offset == self.contents.len() {
                    false
                } else {
                    self.contents.truncate(self.offset);
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

    /// Whether or not the prompt is empty.
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Return the prompt contents as well as an 'offset' which is required in the presence of an
    /// initial grapheme that is too large to fit at the beginning of the screen.
    pub fn view(&self) -> (&str, u16) {
        if self.width == 0 {
            return ("", 0);
        }

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
    pub fn resize(&mut self, width: u16) {
        self.width = width;

        let padding = self.padding();
        let capacity = width - padding;
        let before = as_u16(self.contents[..self.offset].width());
        let after = as_u16(self.contents[self.offset..].width());

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

    /// Get the contents of the prompt.
    pub fn contents(&self) -> &str {
        &self.contents
    }

    /// Reset the prompt, moving the cursor to the end.
    pub fn set_query<Q: Into<String>>(&mut self, prompt: Q) -> bool {
        let mut contents = prompt.into();
        normalize_prompt_string(&mut contents);
        let contents_changed = contents != self.contents;
        self.contents = contents;
        self.offset = self.contents.len();
        self.screen_offset = as_u16(self.contents.width()).min(self.width - self.padding());
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
    fn insert_char(&mut self, ch: char) {
        let mut encoded = [0; 4];
        self.insert(ch.encode_utf8(&mut encoded));
    }

    /// Insert a string at the cursor position.
    fn insert(&mut self, string: &str) {
        let previous_grapheme = self.contents[..self.offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(self.offset, |(offset, _)| offset);
        let old_width = self.contents[previous_grapheme..self.offset].width();

        self.contents.insert_str(self.offset, string);
        self.offset += string.len();

        let new_width = self.contents[previous_grapheme..self.offset].width();
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
        let mut graphemes = self.contents[..self.offset].graphemes(true).rev();
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
    fn move_cursor(&mut self, cm: CursorMovement) -> bool {
        match cm {
            CursorMovement::Left(n) => {
                let new_offset = self.offset.left(&self.contents, n);
                if new_offset != self.offset {
                    let step_width = self.contents[new_offset..self.offset].width();
                    self.offset = new_offset;
                    self.left_by(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::WordLeft(n) => {
                let new_offset = self.offset.left_word(&self.contents, n);
                if new_offset != self.offset {
                    let step_width = self.contents[new_offset..self.offset].width();
                    self.offset = new_offset;
                    self.left_by(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::Right(n) => {
                let new_offset = self.offset.right(&self.contents, n);
                if new_offset != self.offset {
                    let step_width = self.contents[self.offset..new_offset].width();
                    self.offset = new_offset;
                    self.right_by(step_width);
                    true
                } else {
                    false
                }
            }
            CursorMovement::WordRight(n) => {
                let new_offset = self.offset.right_word(&self.contents, n);
                if new_offset != self.offset {
                    let step_width = self.contents[self.offset..new_offset].width();
                    self.offset = new_offset;
                    self.right_by(step_width);
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
                    let max_offset = self.width - self.padding();
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
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PromptStatus {
    pub needs_redraw: bool,
    pub contents_changed: bool,
}

impl ComponentStatus for PromptStatus {
    fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }
}

impl std::ops::BitOrAssign for PromptStatus {
    fn bitor_assign(&mut self, rhs: Self) {
        self.needs_redraw |= rhs.needs_redraw;
        self.contents_changed |= rhs.contents_changed;
    }
}

impl Prompt {
    pub fn draw<W: std::io::Write + ?Sized>(
        &mut self,
        width: u16,
        _height: u16,
        writer: &mut W,
    ) -> std::io::Result<()> {
        use crossterm::{
            QueueableCommand,
            cursor::MoveRight,
            style::Print,
            terminal::{Clear, ClearType},
        };

        writer.queue(Print(">"))?;

        if let Some(width) = width.checked_sub(2) {
            writer.queue(Print(" "))?;
            if width != self.width {
                self.resize(width);
            }

            let (contents, shift) = self.view();

            if shift != 0 {
                writer.queue(MoveRight(shift))?;
            }

            writer
                .queue(Print(contents))?
                .queue(Clear(ClearType::UntilNewLine))?;
        }

        Ok(())
    }
}
