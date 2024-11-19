//! # Terminal renderer
//! This module contains the main representation of the internal state of the picker, as well as
//! the code for rendering the picker to a terminal screen.

#![allow(clippy::cast_possible_truncation)]

mod item;
mod span;
mod unicode;

use std::{
    cmp::min,
    io::{self, StderrLock, Write},
    ops::Range,
    time::Duration,
};

use crossterm::{
    cursor::{MoveTo, MoveToColumn, MoveToPreviousLine, MoveUp},
    event::{poll, read},
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    ExecutableCommand, QueueableCommand,
};
use nucleo::{
    pattern::{CaseMatching, Normalization},
    Matcher,
};

use self::{
    item::{new_rendered, Rendered},
    span::Spanned,
    unicode::{AsciiProcessor, Processor, Span, UnicodeProcessor},
};
use crate::{
    bind::{convert, Event},
    component::{Edit, EditableString},
    Render,
};

/// The outcome after processing all of the events.
pub enum EventSummary {
    /// Continue rendering the frame.
    Continue,
    /// The prompt was updated; were the updates append-only?
    UpdatePrompt(bool),
    /// Select the given item and quit.
    Select,
    /// Quit without selecting an item.
    Quit,
}

/// The dimension parameters of various items in the screen.
#[derive(Debug)]
struct Dimensions {
    /// The width of the screen.
    width: u16,
    /// The height of the screen, including the prompt.
    height: u16,
    /// The left buffer size of the prompt.
    prompt_left_padding: u16,
    /// The right buffer size of the prompt.
    prompt_right_padding: u16,
}

impl Dimensions {
    /// Initialize based on screen dimensions.
    pub fn from_screen(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            prompt_left_padding: width / 8,
            prompt_right_padding: width / 12,
        }
    }

    pub fn move_to_end_of_line(&self) -> MoveToColumn {
        MoveToColumn(self.width - 1)
    }

    /// The [`MoveTo`] command for setting the cursor at the bottom left corner of the match
    /// printing area.
    pub fn move_to_results_start(&self) -> MoveTo {
        MoveTo(0, self.max_draw_count())
    }

    /// The maximum width of the prompt string display window.
    pub fn prompt_max_width(&self) -> usize {
        self.width
            .saturating_sub(self.prompt_left_padding)
            .saturating_sub(self.prompt_right_padding)
            .saturating_sub(2) as _
    }

    /// The maximum number of matches which can be drawn to the screen.
    pub fn max_draw_count(&self) -> u16 {
        self.height.saturating_sub(2)
    }

    /// The maximum length on which a match can be drawn.
    pub fn max_draw_length(&self) -> u16 {
        self.width.saturating_sub(2)
    }

    /// The y index of the prompt string.
    fn prompt_y(&self) -> u16 {
        self.height.saturating_sub(1)
    }

    /// The command to move to the start of the prompt rendering region.
    pub fn move_to_prompt(&self) -> MoveTo {
        MoveTo(0, self.prompt_y())
    }

    /// The command to move to the cursor position.
    pub fn move_to_cursor(&self, view_position: usize) -> MoveTo {
        MoveTo((view_position + 2) as _, self.prompt_y())
    }
}

/// Configuration used internally in the [`PickerState`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PickerConfig {
    pub highlight: bool,
    pub case_matching: CaseMatching,
    pub normalization: Normalization,
    pub right_highlight_buffer: u16,
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            highlight: true,
            case_matching: CaseMatching::Smart,
            normalization: Normalization::Smart,
            right_highlight_buffer: 3,
        }
    }
}

/// The struct which draws the content to the screen.
#[derive(Debug)]
pub struct Compositor<'a> {
    /// The dimensions of the terminal window.
    dimensions: Dimensions,
    /// The selector index position, or [`None`] if there is nothing to select.
    selector_index: Option<u16>,
    /// The prompt string.
    prompt: EditableString,
    /// The current number of items to be drawn to the terminal.
    draw_count: u16,
    /// The total number of items.
    item_count: u32,
    /// The number of matches.
    matched_item_count: u32,
    /// Has the state changed?
    needs_redraw: bool,
    /// Configuration for drawing the picker.
    config: &'a PickerConfig,
    /// A reusable buffer for spans used to render items.
    spans: Vec<Span>,
    /// A reusable buffer of sub-slices of `spans`.
    lines: Vec<Range<usize>>,
    /// A resuable buffer for indices generated from a match.
    indices: Vec<u32>,
}

impl<'a> Compositor<'a> {
    /// The initial state.
    pub fn new(screen: (u16, u16), config: &'a PickerConfig) -> Self {
        let dimensions = Dimensions::from_screen(screen.0, screen.1);
        let prompt = EditableString::new(dimensions.prompt_max_width());

        Self {
            dimensions,
            selector_index: None,
            prompt,
            draw_count: 0,
            matched_item_count: 0,
            item_count: 0,
            needs_redraw: true,
            config,
            spans: Vec::with_capacity(16),
            lines: Vec::with_capacity(4),
            indices: Vec::with_capacity(16),
        }
    }

    /// Return the current index of the selection, if any.
    pub fn selection(&self) -> Option<u16> {
        self.selector_index
    }

    /// Increment the current item selection.
    fn incr_selection(&mut self) {
        self.needs_redraw = true;
        self.selector_index = self.selector_index.map(|i| i.saturating_add(1));
        self.clamp_selector_index();
    }

    /// Decrement the current item selection.
    fn decr_selection(&mut self) {
        self.needs_redraw = true;
        self.selector_index = self.selector_index.map(|i| i.saturating_sub(1));
        self.clamp_selector_index();
    }

    /// Update the draw count from a snapshot.
    pub fn update<T: Send + Sync + 'static>(
        &mut self,
        changed: bool,
        snapshot: &nucleo::Snapshot<T>,
    ) {
        if changed {
            self.needs_redraw = true;
            self.item_count = snapshot.item_count();
            self.matched_item_count = snapshot.matched_item_count();
            self.draw_count = self.matched_item_count.try_into().unwrap_or(u16::MAX);
            self.clamp_draw_count();
            self.clamp_selector_index();
        }
    }

    /// Clamp the draw count so that it falls in the valid range.
    fn clamp_draw_count(&mut self) {
        self.draw_count = min(self.draw_count, self.dimensions.max_draw_count());
    }

    /// Clamp the selector index so that it falls in the valid range.
    fn clamp_selector_index(&mut self) {
        if self.draw_count == 0 {
            self.selector_index = None;
        } else {
            let position = min(self.selector_index.unwrap_or(0), self.draw_count - 1);
            self.selector_index = Some(position);
        }
    }

    /// Perform the given edit action.
    fn edit_prompt(&mut self, st: Edit) {
        self.needs_redraw |= self.prompt.edit(st);
    }

    /// Set the prompt to a given string, moving the cursor to the end.
    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt.set_prompt(prompt);
        self.needs_redraw = true;
    }

    /// The current contents of the prompt.
    pub fn prompt_contents(&self) -> String {
        self.prompt.full_contents()
    }

    /// Clear the queued events.
    pub fn handle(&mut self) -> Result<EventSummary, io::Error> {
        let mut update_prompt = false;
        let mut append = true;

        while poll(Duration::from_millis(5))? {
            if let Some(event) = convert(read()?) {
                match event {
                    Event::Abort => {
                        return Err(io::Error::new(io::ErrorKind::Other, "keyboard interrupt"))
                    }
                    Event::MoveToStart => self.edit_prompt(Edit::ToStart),
                    Event::MoveToEnd => self.edit_prompt(Edit::ToEnd),
                    Event::Insert(ch) => {
                        update_prompt = true;
                        append &= self.prompt.is_appending();
                        self.edit_prompt(Edit::Insert(ch));
                    }
                    Event::Select => return Ok(EventSummary::Select),
                    Event::MoveUp => self.incr_selection(),
                    Event::MoveDown => self.decr_selection(),
                    Event::MoveLeft => self.edit_prompt(Edit::Left),
                    Event::MoveRight => self.edit_prompt(Edit::Right),
                    Event::Delete => {
                        if !self.prompt.is_empty() {
                            update_prompt = true;
                            append = false;
                            self.edit_prompt(Edit::Delete);
                        }
                    }
                    Event::Quit => return Ok(EventSummary::Quit),
                    Event::Resize(width, height) => {
                        self.resize(width, height);
                    }
                    Event::Paste(contents) => {
                        update_prompt = true;
                        append &= self.prompt.is_appending();
                        self.edit_prompt(Edit::Paste(contents));
                    }
                }
            }
        }
        Ok(if update_prompt {
            EventSummary::UpdatePrompt(append)
        } else {
            EventSummary::Continue
        })
    }

    #[inline]
    fn draw_match<P: Processor>(
        &mut self,
        stderr: &mut StderrLock<'_>,
        rendered: &str,
        current_draw_count: &mut usize,
        idx: usize,
    ) -> Result<bool, io::Error> {
        // convert the indices into spans
        let spanned: Spanned<'_, P> =
            Spanned::new(&self.indices, rendered, &mut self.spans, &mut self.lines);

        // space needed to render the next entry
        let required_headspace = spanned.count_lines();
        *current_draw_count += required_headspace;

        // not enough space: bail early
        if *current_draw_count > self.dimensions.max_draw_count() as usize {
            return Ok(true);
        }
        // since max_draw_count() returns a u16, if required_headspace
        // does not fit, it would have already exited in the previous line
        let required_headspace = required_headspace as u16;

        // move the cursor up the appropriate amount and then print the selection to the
        // terminal
        stderr.queue(MoveToPreviousLine(required_headspace))?;
        spanned.queue_print(
            stderr,
            self.selector_index.is_some_and(|i| i as usize == idx),
            self.dimensions.max_draw_length(),
            self.config.right_highlight_buffer,
        )?;

        // fix the cursor position
        stderr.queue(MoveToPreviousLine(required_headspace))?;
        Ok(false)
    }

    /// Draw the terminal to the screen. This assumes that the draw count has been updated and the
    /// selector index has been properly clamped, or this method will panic!
    pub fn draw<T: Send + Sync + 'static, R: Render<T>>(
        &mut self,
        stderr: &mut StderrLock<'_>,
        matcher: &mut Matcher,
        render: &R,
        snapshot: &nucleo::Snapshot<T>,
    ) -> Result<(), io::Error> {
        if self.needs_redraw {
            // reset redraw state
            self.needs_redraw = false;

            // we can't do anything if the screen is so narrow
            if self.dimensions.width < 4 {
                stderr.queue(Clear(ClearType::All))?.flush()?;
                return Ok(());
            }

            stderr.execute(BeginSynchronizedUpdate)?;

            // draw the match counts
            stderr.queue(self.dimensions.move_to_results_start())?;
            stderr
                .queue(SetAttribute(Attribute::Italic))?
                .queue(SetForegroundColor(Color::Green))?
                .queue(Print("  "))?
                .queue(Print(self.matched_item_count))?
                .queue(Print("/"))?
                .queue(Print(self.item_count))?
                .queue(SetAttribute(Attribute::Reset))?
                .queue(ResetColor)?
                .queue(Clear(ClearType::UntilNewLine))?;

            let mut current_draw_count = 0;

            // draw the matches
            for (idx, it) in snapshot.matched_items(..).enumerate() {
                // generate the indices
                if self.config.highlight {
                    self.indices.clear();
                    snapshot.pattern().column_pattern(0).indices(
                        it.matcher_columns[0].slice(..),
                        matcher,
                        &mut self.indices,
                    );
                    self.indices.sort_unstable();
                    self.indices.dedup();
                }

                match new_rendered(&it, render) {
                    Rendered::Ascii(s) => {
                        if self.draw_match::<AsciiProcessor>(
                            stderr,
                            s,
                            &mut current_draw_count,
                            idx,
                        )? {
                            break;
                        }
                    }
                    Rendered::Unicode(r) => {
                        if self.draw_match::<UnicodeProcessor>(
                            stderr,
                            r.as_ref(),
                            &mut current_draw_count,
                            idx,
                        )? {
                            break;
                        }
                    }
                }
            }

            // clear above the current matches
            if self.draw_count != self.dimensions.max_draw_count() {
                stderr
                    .queue(MoveUp(1))?
                    .queue(self.dimensions.move_to_end_of_line())?
                    .queue(Clear(ClearType::FromCursorUp))?;
            }

            // render the prompt string
            let view = self.prompt.view_padded(
                self.dimensions.prompt_left_padding as _,
                self.dimensions.prompt_right_padding as _,
            );
            stderr
                .queue(self.dimensions.move_to_prompt())?
                .queue(Print("> "))?
                .queue(Print(&view))?
                .queue(Clear(ClearType::UntilNewLine))?
                .queue(self.dimensions.move_to_cursor(view.index()))?;

            // flush to terminal
            stderr.flush()?;
            stderr.execute(EndSynchronizedUpdate)?;
            Ok(())
        } else {
            Ok(())
        }
    }

    /// Resize the terminal state on screen size change.
    fn resize(&mut self, width: u16, height: u16) {
        self.needs_redraw = true;
        self.dimensions = Dimensions::from_screen(width, height);
        self.prompt.resize(self.dimensions.prompt_max_width());
        self.clamp_draw_count();
        self.clamp_selector_index();
    }
}
