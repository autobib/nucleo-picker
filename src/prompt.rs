#[cfg(test)]
mod tests;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    component::{Component, Status},
    util::as_u16,
};

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
    /// Move the cursor to the start.
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
    /// Insert a character at the cursor position.
    Insert(char),
    /// Paste a string at the cursor position.
    Paste(String),
    /// Set the prompt to the value at the string and move the cursor to the end.
    #[allow(unused)]
    Set(String),
}

impl PromptEvent {
    pub fn is_cursor_movement(&self) -> bool {
        matches!(
            &self,
            PromptEvent::Left(_)
                | PromptEvent::WordLeft(_)
                | PromptEvent::Right(_)
                | PromptEvent::WordRight(_)
                | PromptEvent::ToStart
                | PromptEvent::ToEnd
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

#[derive(Debug)]
pub struct PromptConfig {
    pub padding: u16,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self { padding: 2 }
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
            width: u16::MAX,
            config,
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
        // TODO: this is not really correct, since it does not handle width 0 correctly.
        // but in practice, for the prompt this is quite rare; but should fix it at some point
        //
        // to witness in tests, set the width to 0 and then to some large value and the screen
        // offset will be incorrect
        //
        // this is also the reason that the prompt defaults to `u16::MAX`; and this should be fixed
        // as well when this is fixed.
        self.width = width;
        self.screen_offset = self.screen_offset.min(width - self.padding());
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
        normalize_prompt_string(&mut self.contents);
        self.offset = self.contents.len();
        self.screen_offset = as_u16(self.contents.width()).min(self.width - self.padding());
    }

    /// Increase the screen offset by the provided width, without exceeding the maximum offset.
    fn right_by(&mut self, width: usize) {
        self.screen_offset = self
            .screen_offset
            .saturating_add(as_u16(width))
            .min(self.width - self.padding());
    }

    /// Insert a character at the cursor position.
    fn insert_char(&mut self, ch: char, w: usize) {
        self.contents.insert(self.offset, ch);
        self.right_by(w);
        self.offset += ch.len_utf8();
    }

    /// Insert a string at the cursor position.
    fn insert(&mut self, string: &str) {
        self.contents.insert_str(self.offset, string);
        self.right_by(string.width());
        self.offset += string.len();
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

impl Status for PromptStatus {
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

impl Component for Prompt {
    type Event = PromptEvent;

    type Status = PromptStatus;

    fn handle(&mut self, e: Self::Event) -> Self::Status {
        let mut contents_changed = false;

        let needs_redraw = match e {
            PromptEvent::Set(s) => {
                self.set_prompt(s);
                true
            }
            PromptEvent::Left(n) => self.move_cursor(CursorMovement::Left(n)),
            PromptEvent::WordLeft(n) => self.move_cursor(CursorMovement::WordLeft(n)),
            PromptEvent::Right(n) => self.move_cursor(CursorMovement::Right(n)),
            PromptEvent::WordRight(n) => self.move_cursor(CursorMovement::WordRight(n)),
            PromptEvent::ToStart => self.move_cursor(CursorMovement::ToStart),
            PromptEvent::ToEnd => self.move_cursor(CursorMovement::ToEnd),
            PromptEvent::Insert(ch) => {
                if let Some((ch, w)) = normalize_char(ch) {
                    contents_changed = true;
                    self.insert_char(ch, w);
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

        Self::Status {
            needs_redraw,
            contents_changed,
        }
    }

    fn draw<W: std::io::Write + ?Sized>(
        &mut self,
        width: u16,
        _height: u16,
        writer: &mut W,
    ) -> std::io::Result<()> {
        use crossterm::{
            cursor::MoveRight,
            style::Print,
            terminal::{Clear, ClearType},
            QueueableCommand,
        };

        writer.queue(Print("> "))?;

        if let Some(width) = width.checked_sub(2) {
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
