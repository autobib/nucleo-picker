//! # Terminal renderer
//! This module contains the main representation of the internal state of the picker, as well as
//! the code for rendering the picker to a terminal screen.

#![allow(clippy::cast_possible_truncation)]

mod editable;
mod item;
mod layout;
mod span;
mod unicode;

use std::{
    io::{self, Write},
    ops::Range,
    time::Duration,
};

use crossterm::{
    cursor::{MoveRight, MoveTo, MoveToColumn, MoveToPreviousLine},
    event::{poll, read},
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    ExecutableCommand, QueueableCommand,
};
use nucleo::{
    pattern::{CaseMatching, Normalization},
    Matcher,
};

pub use self::editable::normalize_query_string;
use self::{
    editable::{Edit, EditableString},
    item::RenderedItem,
    layout::{Layout, VariableSizeBuffer},
    span::{Head, KeepLines, Spanned, Tail},
    unicode::{AsciiProcessor, Span, UnicodeProcessor},
};
use crate::{
    bind::{convert, Event},
    // component::{Edit, EditableString},
    Render,
};

const ELLIPSIS: char = '…';

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
    /// The padding at the bottom.
    scroll_padding_bottom: u16,
    /// The padding at the top.
    scroll_padding_top: u16,
}

impl Dimensions {
    /// Initialize based on screen dimensions.
    pub fn from_screen(config: &PickerConfig, width: u16, height: u16) -> Self {
        let scroll_padding = config.scroll_padding.min(height.saturating_sub(3) / 2);
        Self {
            width,
            height,
            scroll_padding_bottom: scroll_padding,
            scroll_padding_top: scroll_padding,
        }
    }

    pub fn move_to_screen_index(&self, index: u16) -> MoveTo {
        MoveTo(0, self.max_draw_height() - 1 - index)
    }

    pub fn move_to_end_of_line(&self) -> MoveToColumn {
        MoveToColumn(self.width - 1)
    }

    /// The [`MoveTo`] command for setting the cursor at the bottom left corner of the match
    /// printing area.
    pub fn move_to_results_start(&self) -> MoveTo {
        MoveTo(0, self.max_draw_height())
    }

    /// The maximum width of the prompt string display window.
    pub fn max_prompt_width(&self) -> u16 {
        self.width.saturating_sub(2)
    }

    /// The maximum number of matches which can be drawn to the screen.
    pub fn max_draw_height(&self) -> u16 {
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
    pub fn move_to_cursor(&self, view_position: u16) -> MoveTo {
        MoveTo(view_position + 2, self.prompt_y())
    }
}

/// Configuration used internally in the [`PickerState`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PickerConfig {
    pub highlight: bool,
    pub case_matching: CaseMatching,
    pub normalization: Normalization,
    pub highlight_padding: u16,
    pub scroll_padding: u16,
    pub prompt_padding: u16,
}

impl Default for PickerConfig {
    fn default() -> Self {
        Self {
            highlight: true,
            case_matching: CaseMatching::Smart,
            normalization: Normalization::Smart,
            highlight_padding: 3,
            scroll_padding: 3,
            prompt_padding: 3,
        }
    }
}

pub struct CompositorBuffer {
    /// Spans used to render items.
    spans: Vec<Span>,
    /// Sub-slices of `spans` corresponding to lines.
    lines: Vec<Range<usize>>,
    /// Indices generated from a match.
    indices: Vec<u32>,
}

impl CompositorBuffer {
    pub fn new() -> Self {
        Self {
            spans: Vec::with_capacity(16),
            lines: Vec::with_capacity(4),
            indices: Vec::with_capacity(16),
        }
    }
}

/// The struct which draws the content to the screen.
#[derive(Debug)]
pub struct Compositor<'a> {
    /// The dimensions of the terminal window.
    dimensions: Dimensions,
    /// The current position of the selection.
    selection: usize,
    /// The prompt string.
    prompt: EditableString,
    /// The total number of items.
    item_count: u32,
    /// The number of matches.
    matched_item_count: u32,
    /// Has the state changed?
    needs_redraw: bool,
    /// Configuration for drawing the picker.
    config: &'a PickerConfig,
    /// Stateful representation of the current screen layout.
    layout: Layout,
}

impl<'a> Compositor<'a> {
    /// The initial state.
    pub fn new(screen: (u16, u16), config: &'a PickerConfig) -> Self {
        let dimensions = Dimensions::from_screen(config, screen.0, screen.1);
        let prompt = EditableString::new(dimensions.max_prompt_width(), config.prompt_padding);

        Self {
            dimensions,
            selection: 0,
            prompt,
            matched_item_count: 0,
            item_count: 0,
            needs_redraw: true,
            config,
            layout: Layout::default(),
        }
    }

    /// Return the current index of the selection, if any.
    #[inline]
    pub fn selection(&self) -> Option<u32> {
        if self.selection < self.matched_item_count as usize {
            Some(self.selection as u32)
        } else {
            None
        }
    }

    /// Increment the current item selection without exceeding the provided bound.
    fn incr_selection(&mut self) {
        if self.selection < self.matched_item_count.saturating_sub(1) as usize {
            self.needs_redraw = true;
            self.selection += 1;
        }
    }

    /// Decrement the current item selection.
    fn decr_selection(&mut self) {
        if let Some(new) = self.selection.checked_sub(1) {
            self.needs_redraw = true;
            self.selection = new;
        }
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
            self.selection = self
                .selection
                .min(self.matched_item_count.saturating_sub(1) as usize);
        }
    }

    /// Perform the given edit action.
    #[inline]
    fn edit_prompt(&mut self, st: Edit) -> bool {
        let changed = self.prompt.edit(st);
        self.needs_redraw |= changed;
        changed
    }

    /// Set the prompt to a given string, moving the cursor to the end.
    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt.set_prompt(prompt);
        self.needs_redraw = true;
    }

    /// The current contents of the prompt.
    pub fn prompt_contents(&self) -> &str {
        self.prompt.contents()
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
                    Event::MoveToStart => {
                        self.edit_prompt(Edit::ToStart);
                    }
                    Event::MoveToEnd => {
                        self.edit_prompt(Edit::ToEnd);
                    }
                    Event::Insert(ch) => {
                        append &= self.prompt.is_appending();
                        update_prompt |= self.edit_prompt(Edit::Insert(ch));
                    }
                    Event::Select => return Ok(EventSummary::Select),
                    Event::MoveUp => self.incr_selection(),
                    Event::MoveDown => self.decr_selection(),
                    Event::MoveLeft => {
                        self.edit_prompt(Edit::Left);
                    }
                    Event::MoveWordLeft => {
                        self.edit_prompt(Edit::WordLeft);
                    }
                    Event::MoveRight => {
                        self.edit_prompt(Edit::Right);
                    }
                    Event::MoveWordRight => {
                        self.edit_prompt(Edit::WordRight);
                    }
                    Event::Backspace => {
                        if self.edit_prompt(Edit::Backspace) {
                            update_prompt = true;
                            append = false;
                        }
                    }
                    Event::BackspaceWord => {
                        if self.edit_prompt(Edit::BackspaceWord) {
                            update_prompt = true;
                            append = false;
                        }
                    }
                    Event::ClearBefore => {
                        if self.edit_prompt(Edit::ClearBefore) {
                            update_prompt = true;
                            append = false;
                        }
                    }
                    Event::Delete => {
                        if self.edit_prompt(Edit::Delete) {
                            update_prompt = true;
                            append = false;
                        }
                    }
                    Event::ClearAfter => {
                        if self.edit_prompt(Edit::ClearAfter) {
                            update_prompt = true;
                            append = false;
                        }
                    }
                    Event::Quit => return Ok(EventSummary::Quit),
                    Event::QuitIfEmpty => {
                        if self.prompt.is_empty() {
                            return Ok(EventSummary::Quit);
                        }
                    }
                    Event::Resize(width, height) => {
                        self.resize(width, height);
                    }
                    Event::Paste(contents) => {
                        append &= self.prompt.is_appending();
                        update_prompt |= self.edit_prompt(Edit::Paste(contents));
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

    /// The inner `match draw` implementation.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn draw_single_match<
        T: Send + Sync + 'static,
        R: Render<T>,
        L: KeepLines,
        W: Write,
        const SELECTED: bool,
    >(
        stderr: &mut W,
        buffer: &mut CompositorBuffer,
        max_draw_length: u16,
        config: &PickerConfig,
        item: &nucleo::Item<'_, T>,
        snapshot: &nucleo::Snapshot<T>,
        matcher: &mut nucleo::Matcher,
        height: u16,
        render: &R,
    ) -> Result<(), io::Error> {
        // generate the indices
        if config.highlight {
            buffer.indices.clear();
            snapshot.pattern().column_pattern(0).indices(
                item.matcher_columns[0].slice(..),
                matcher,
                &mut buffer.indices,
            );
            buffer.indices.sort_unstable();
            buffer.indices.dedup();
        }

        match RenderedItem::new(item, render) {
            RenderedItem::Ascii(s) => Spanned::<'_, AsciiProcessor>::new(
                &buffer.indices,
                s,
                &mut buffer.spans,
                &mut buffer.lines,
                L::from_offset(height),
            )
            .queue_print(stderr, SELECTED, max_draw_length, config.highlight_padding),
            RenderedItem::Unicode(r) => Spanned::<'_, UnicodeProcessor>::new(
                &buffer.indices,
                r.as_ref(),
                &mut buffer.spans,
                &mut buffer.lines,
                L::from_offset(height),
            )
            .queue_print(stderr, SELECTED, max_draw_length, config.highlight_padding),
        }
    }

    #[inline]
    fn draw_matches<T: Send + Sync + 'static, R: Render<T>, W: Write>(
        &mut self,
        stderr: &mut W,
        matcher: &mut Matcher,
        render: &R,
        snapshot: &nucleo::Snapshot<T>,
        buffer: &mut CompositorBuffer,
    ) -> Result<(), io::Error> {
        // draw the matches
        if snapshot.matched_item_count() == 0 {
            // erase the matches if there are no matched items
            stderr
                .queue(MoveToPreviousLine(1))?
                .queue(self.dimensions.move_to_end_of_line())?
                .queue(Clear(ClearType::FromCursorUp))?;
        } else {
            // recompute the layout
            let view = self.layout.recompute(
                self.dimensions.max_draw_height(),
                self.dimensions.scroll_padding_bottom,
                self.dimensions.scroll_padding_top,
                self.selection as u32,
                snapshot,
            );

            let mut match_lines_rendered = 0;
            let mut item_iter = snapshot.matched_items(
                self.selection as u32 + 1 - view.below.len() as u32
                    ..=self.selection as u32 + view.above.len() as u32,
            );

            // render below the selection
            for height in view.below[1..].iter().rev() {
                match_lines_rendered += height;
                stderr.queue(
                    self.dimensions
                        .move_to_screen_index(match_lines_rendered - 1),
                )?;

                Self::draw_single_match::<T, R, Head, W, false>(
                    stderr,
                    buffer,
                    self.dimensions.max_draw_length(),
                    self.config,
                    &item_iter.next().unwrap(),
                    snapshot,
                    matcher,
                    *height,
                    render,
                )?;
            }

            // render the selection
            match_lines_rendered += view.below[0];
            stderr.queue(
                self.dimensions
                    .move_to_screen_index(match_lines_rendered - 1),
            )?;

            Self::draw_single_match::<T, R, Head, W, true>(
                stderr,
                buffer,
                self.dimensions.max_draw_length(),
                self.config,
                &item_iter.next().unwrap(),
                snapshot,
                matcher,
                view.below[0],
                render,
            )?;

            // render above the selection
            for height in view.above {
                match_lines_rendered += height;
                stderr.queue(
                    self.dimensions
                        .move_to_screen_index(match_lines_rendered - 1),
                )?;

                Self::draw_single_match::<T, R, Tail, W, false>(
                    stderr,
                    buffer,
                    self.dimensions.max_draw_length(),
                    self.config,
                    &item_iter.next().unwrap(),
                    snapshot,
                    matcher,
                    *height,
                    render,
                )?;
            }

            // clear above matches if required
            if match_lines_rendered + 1 < self.dimensions.max_draw_height() {
                stderr
                    .queue(self.dimensions.move_to_screen_index(match_lines_rendered))?
                    .queue(self.dimensions.move_to_end_of_line())?
                    .queue(Clear(ClearType::FromCursorUp))?;
            }
        }

        Ok(())
    }

    /// Draw the prompt string
    fn draw_prompt<W: Write>(&self, stderr: &mut W) -> Result<(), io::Error> {
        let (contents, shift) = self.prompt.view();

        stderr
            .queue(self.dimensions.move_to_prompt())?
            .queue(Print("> "))?;

        if shift != 0 {
            stderr.queue(MoveRight(shift))?;
        }

        stderr
            .queue(Print(contents))?
            .queue(Clear(ClearType::UntilNewLine))?
            .queue(self.dimensions.move_to_cursor(self.prompt.screen_offset()))?;

        Ok(())
    }

    /// Draw the match counts to the terminal, e.g. `9/43`.
    fn draw_match_counts<W: Write>(&mut self, writer: &mut W) -> Result<(), io::Error> {
        writer.queue(self.dimensions.move_to_results_start())?;
        writer
            .queue(SetAttribute(Attribute::Italic))?
            .queue(SetForegroundColor(Color::Green))?
            .queue(Print("  "))?
            .queue(Print(self.matched_item_count))?
            .queue(Print("/"))?
            .queue(Print(self.item_count))?
            .queue(SetAttribute(Attribute::Reset))?
            .queue(ResetColor)?
            .queue(Clear(ClearType::UntilNewLine))?;
        Ok(())
    }

    /// Draw the terminal to the screen. This assumes that the draw count has been updated and the
    /// selector index has been properly clamped, or this method will panic!
    pub fn draw<T: Send + Sync + 'static, R: Render<T>, W: Write>(
        &mut self,
        writer: &mut W,
        matcher: &mut Matcher,
        render: &R,
        snapshot: &nucleo::Snapshot<T>,
        buffer: &mut CompositorBuffer,
    ) -> Result<(), io::Error> {
        if self.needs_redraw {
            // reset redraw state
            self.needs_redraw = false;

            writer.execute(BeginSynchronizedUpdate)?;

            // draw the match counts
            self.draw_match_counts(writer)?;

            // draw matches if there is space; the height check is required otherwise the
            // `recompute` function will panic
            if self.dimensions.max_draw_height() != 0 {
                self.draw_matches(writer, matcher, render, snapshot, buffer)?;
            }

            // render the prompt string
            self.draw_prompt(writer)?;

            // flush to terminal
            writer.flush()?;
            writer.execute(EndSynchronizedUpdate)?;
        };

        Ok(())
    }

    /// Resize the terminal state on screen size change.
    fn resize(&mut self, width: u16, height: u16) {
        self.needs_redraw = true;
        self.dimensions = Dimensions::from_screen(self.config, width, height);
        self.prompt.resize(
            self.dimensions.max_prompt_width(),
            self.config.prompt_padding,
        );
    }
}
