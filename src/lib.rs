//! # A generic fuzzy item picker
//! This is a generic picker implementation which wraps the [`nucleo::Nucleo`] matching engine with
//! an interactive TUI.
//!
//! The API is pretty similar to that exposed by the [`nucleo`] crate; the majority of the internal state of [`Nucleo`] is re-exposed through the main [`Picker`] entrypoint.
//!
//! For usage examples, visit the [examples
//! folder](https://github.com/autobib/nucleo-picker/tree/master/examples) on GitHub.
mod bind;
pub mod component;
pub mod fill;

use std::{
    cmp::min,
    io::{self, Stdout, Write},
    process::exit,
    sync::Arc,
    thread::{available_parallelism, sleep},
    time::{Duration, Instant},
};

use crossterm::{
    cursor::{MoveTo, MoveToColumn, MoveToPreviousLine, MoveUp},
    event::{poll, read, DisableBracketedPaste, EnableBracketedPaste},
    execute,
    style::{
        Attribute, Color, Print, PrintStyledContent, ResetColor, SetAttribute, SetForegroundColor,
        Stylize,
    },
    terminal::{
        disable_raw_mode, enable_raw_mode, size, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
    tty::IsTty,
    QueueableCommand,
};
use nucleo::{Config, Injector, Nucleo, Utf32String};

use crate::{
    bind::{convert, Event},
    component::{Edit, EditableString},
};

pub use nucleo;

/// The outcome after processing all of the events.
enum EventSummary {
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
struct PickerState {
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

    /// Increment the current item selection.
    pub fn incr_selection(&mut self) {
        self.needs_redraw = true;
        self.selector_index = self.selector_index.map(|i| i.saturating_add(1));
        self.clamp_selector_index();
    }

    /// Decrement the current item selection.
    pub fn decr_selection(&mut self) {
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
        self.draw_count = min(self.draw_count, self.dimensions.max_draw_count())
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
    pub fn edit_prompt(&mut self, st: Edit) {
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

    /// Clear the queued events.
    fn handle(&mut self) -> Result<EventSummary, io::Error> {
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
    pub fn resize(&mut self, width: u16, height: u16) {
        self.needs_redraw = true;
        self.dimensions = Dimensions::from_screen(width, height);
        self.prompt.resize(self.dimensions.prompt_max_width());
        self.clamp_draw_count();
        self.clamp_selector_index();
    }
}

/// # Options for a picker
/// Specify configuration options for a [`Picker`] before initialization.
pub struct PickerOptions {
    columns: u32,
    config: Config,
    query: Option<String>,
    threads: Option<usize>,
}

impl Default for PickerOptions {
    fn default() -> Self {
        Self {
            columns: 1,
            config: Config::DEFAULT,
            query: None,
            threads: None,
        }
    }
}

impl PickerOptions {
    /// Initialize with default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of columns.
    pub fn columns(&mut self, columns: u32) -> &mut Self {
        self.columns = columns;
        self
    }

    /// Set the internal matcher configuration.
    pub fn config(&mut self, config: Config) -> &mut Self {
        self.config = config;
        self
    }

    /// Provide a default query string.
    pub fn query<Q: ToString>(&mut self, query: Q) -> &mut Self {
        self.query = Some(query.to_string());
        self
    }

    /// Convert into a [`Picker`].
    pub fn picker<T: Send + Sync + 'static>(self) -> Picker<T> {
        let matcher = Nucleo::new(
            self.config,
            Arc::new(|| {}),
            self.threads.or_else(|| {
                // Reserve two threads:
                // 1. for populating the macher
                // 2. for rendering the terminal UI and handling user input
                available_parallelism()
                    .map(|it| it.get().checked_sub(2).unwrap_or(1))
                    .ok()
            }),
            self.columns,
        );

        Picker {
            matcher,
            query: self.query,
        }
    }
}

/// # The core item picker
/// This is the main entrypoint for this crate. Initialize a picker with [`Picker::default`], or
/// with custom configuration using [`PickerOptions`], and add elements to the picker using an [`Injector`]
/// returned by the [`Picker::injector`] method.
///
/// See also the documentation for [`nucleo::Nucleo`] and [`nucleo::Injector`], or the
/// [usage examples](https://github.com/autobib/nucleo-picker/tree/master/examples).
pub struct Picker<T: Send + Sync + 'static> {
    matcher: Nucleo<T>,
    query: Option<String>,
}

impl<T: Send + Sync + 'static> Default for Picker<T> {
    fn default() -> Self {
        PickerOptions::default().picker()
    }
}

impl<T: Send + Sync + 'static> Picker<T> {
    /// Default frame interval of 16ms, or ~60 FPS.
    const fn default_frame_interval() -> Duration {
        Duration::from_millis(16)
    }

    /// Convenience method to initialize a picker with a single column and the provided nucleo
    /// configuration.
    pub fn with_config(config: Config) -> Self {
        let mut opts = PickerOptions::default();
        opts.config(config);
        opts.picker()
    }

    /// Update the default query string to a provided value. This is mainly useful for modifying the
    /// query string before re-using the [`Picker`].
    ///
    /// See also the [`PickerOptions::query`] method to set the query during initialization.
    pub fn update_query(&mut self, query: String) {
        self.query = Some(query);
    }

    /// Get an [`Injector`] from the internal [`Nucleo`] instance.
    pub fn injector(&self) -> Injector<T> {
        self.matcher.injector()
    }

    /// Open the interactive picker prompt and return the picked item, if any.
    pub fn pick(&mut self) -> Result<Option<&T>, io::Error> {
        if !std::io::stdin().is_tty() {
            return Err(io::Error::new(io::ErrorKind::Other, "is not interactive"));
        }

        self.pick_inner(Self::default_frame_interval())
    }

    /// The actual picker implementation.
    fn pick_inner(&mut self, interval: Duration) -> Result<Option<&T>, io::Error> {
        let mut stdout = io::stdout();
        let mut term = PickerState::new(size()?);
        if let Some(query) = self.query.as_ref() {
            term.set_prompt(query);
        }

        enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;

        let selection = loop {
            let deadline = Instant::now() + interval;

            // process any queued keyboard events and reset pattern if necessary
            match term.handle()? {
                EventSummary::Continue => {}
                EventSummary::UpdatePrompt(append) => {
                    self.matcher.pattern.reparse(
                        0,
                        &term.prompt.full_contents(),
                        nucleo::pattern::CaseMatching::Smart,
                        nucleo::pattern::Normalization::Smart,
                        append,
                    );
                }
                EventSummary::Select => {
                    break term
                        .selector_index
                        .and_then(|idx| self.matcher.snapshot().get_matched_item(idx as _))
                        .map(|it| it.data);
                }
                EventSummary::Quit => {
                    break None;
                }
            };

            // increment the matcher and update state
            let status = self.matcher.tick(10);
            term.update(status.changed, self.matcher.snapshot());

            // redraw the screen
            term.draw(&mut stdout, self.matcher.snapshot())?;

            // wait if frame rendering finishes early
            sleep(deadline - Instant::now());
        };

        disable_raw_mode()?;
        execute!(stdout, DisableBracketedPaste, LeaveAlternateScreen)?;
        Ok(selection)
    }
}
