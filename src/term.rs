//! # Terminal renderer
//! This module contains the main representation of the internal state of the picker, as well as
//! the code for rendering the picker to a terminal screen.
use std::{
    cmp::min,
    io::{self, Stdout, Write},
    process::exit,
    time::Duration,
};

use crossterm::{
    cursor::{MoveTo, MoveToColumn, MoveToPreviousLine, MoveUp},
    event::{poll, read},
    style::{
        Attribute, Color, Print, PrintStyledContent, ResetColor, SetAttribute, SetForegroundColor,
        Stylize,
    },
    terminal::{Clear, ClearType},
    QueueableCommand,
};
use nucleo::Utf32String;

use crate::{
    bind::{convert, Event},
    component::{Edit, EditableString},
};

/// The outcome after processing all of the events.
pub enum EventSummary {
    /// Continue rendering the frame.
    Continue,
    /// The prompt was updated; where the updates append-only?
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

/// A representation of the current state of the picker.
#[derive(Debug)]
pub struct PickerState {
    /// The dimensions of the application.
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
}

impl PickerState {
    /// The initial picker state.
    pub fn new(screen: (u16, u16)) -> Self {
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

    /// Set the prompt to a given string, moving the cursor to the beginning.
    pub fn set_prompt(&mut self, prompt: &str) {
        self.prompt.set_prompt(prompt);
        self.needs_redraw = true;
    }

    /// Format a [`Utf32String`] for displaying. Currently:
    /// - Delete control characters.
    /// - Truncates the string to an appropriate length.
    /// - Replaces any newline characters with spaces.
    fn format_display(&self, display: &Utf32String) -> String {
        display
            .slice(..)
            .chars()
            .filter(|ch| !ch.is_control())
            .take(self.dimensions.max_draw_length() as _)
            .map(|ch| match ch {
                '\n' => ' ',
                s => s,
            })
            .collect()
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
                    Event::Abort => exit(1),
                    Event::MoveToStart => self.edit_prompt(Edit::ToStart),
                    Event::MoveToEnd => self.edit_prompt(Edit::ToEnd),
                    Event::Insert(ch) => {
                        update_prompt = true;
                        // if the cursor is at the end, it means the character was appended
                        append &= self.prompt.cursor_at_end();
                        self.edit_prompt(Edit::Insert(ch));
                    }
                    Event::Select => return Ok(EventSummary::Select),
                    Event::MoveUp => self.incr_selection(),
                    Event::MoveDown => self.decr_selection(),
                    Event::MoveLeft => self.edit_prompt(Edit::Left),
                    Event::MoveRight => self.edit_prompt(Edit::Right),
                    Event::Delete => {
                        update_prompt = true;
                        append = false;
                        self.edit_prompt(Edit::Delete);
                    }
                    Event::Quit => return Ok(EventSummary::Quit),
                    Event::Resize(width, height) => {
                        self.resize(width, height);
                    }
                    Event::Paste(contents) => {
                        update_prompt = true;
                        append &= self.prompt.cursor_at_end();
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

    /// Draw the terminal to the screen. This assumes that the draw count has been updated and the
    /// selector index has been properly clamped, or this method will panic!
    pub fn draw<T: Send + Sync + 'static>(
        &mut self,
        stdout: &mut Stdout,
        snapshot: &nucleo::Snapshot<T>,
    ) -> Result<(), io::Error> {
        if self.needs_redraw {
            // reset redraw state
            self.needs_redraw = false;

            // draw the match counts
            stdout.queue(self.dimensions.move_to_results_start())?;
            stdout
                .queue(SetAttribute(Attribute::Italic))?
                .queue(SetForegroundColor(Color::Green))?
                .queue(Print("  "))?
                .queue(Print(self.matched_item_count))?
                .queue(Print("/"))?
                .queue(Print(self.item_count))?
                .queue(SetAttribute(Attribute::Reset))?
                .queue(ResetColor)?
                .queue(Clear(ClearType::UntilNewLine))?;

            // draw the matches
            for (idx, it) in snapshot.matched_items(..self.draw_count as u32).enumerate() {
                let render = self.format_display(&it.matcher_columns[0]);
                if Some(idx) == self.selector_index.map(|i| i as _) {
                    stdout
                        .queue(MoveToPreviousLine(1))?
                        .queue(SetAttribute(Attribute::Bold))?
                        .queue(PrintStyledContent("▌ ".with(Color::Magenta)))? // selection indicator
                        .queue(Print(render))?
                        .queue(SetAttribute(Attribute::Reset))?
                        .queue(Clear(ClearType::UntilNewLine))?;
                } else {
                    stdout
                        .queue(MoveToPreviousLine(1))?
                        .queue(Print("  "))?
                        .queue(Print(render))?
                        .queue(Clear(ClearType::UntilNewLine))?;
                }
            }

            // clear above the current matches
            if self.draw_count != self.dimensions.max_draw_count() {
                stdout
                    .queue(MoveUp(1))?
                    .queue(self.dimensions.move_to_end_of_line())?
                    .queue(Clear(ClearType::FromCursorUp))?;
            }

            // render the prompt string
            let view = self.prompt.view_padded(
                self.dimensions.prompt_left_padding as _,
                self.dimensions.prompt_right_padding as _,
            );
            stdout
                .queue(self.dimensions.move_to_prompt())?
                .queue(Print("> "))?
                .queue(Print(&view))?
                .queue(Clear(ClearType::UntilNewLine))?
                .queue(self.dimensions.move_to_cursor(view.index()))?;

            // flush to terminal
            stdout.flush()
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
