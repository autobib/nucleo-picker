use std::ops::Range;

use crate::draw::Style;

#[cfg(feature = "preview-ansi")]
use super::AnsiWriter;
use super::{Buffer, TextWriter};
use crate::{
    session::Dimensions,
    util::unicode::{Processor, UnicodeProcessor},
};

/// A fixed view into a preview buffer.
///
/// This is a wrapper type around a [preview buffer](Buffer); the internal buffer can be
/// accessed with [`buffer`](Self::buffer) and [`buffer_mut`](Self::buffer_mut). Some methods are
/// forwarded for convenience.
///
/// Essentially, it consists of the buffer itself, an offset (the coordinates of the top left corner),
/// state concerning line numbers, and the error status. Most notably, this is an unsized view.
/// Manipulating the view requires information about the size, provided when constructing a
/// [viewport](Self::viewport).
#[derive(Debug, Default)]
pub struct Pane {
    buffer: Buffer,
    offset: (usize, usize),
    line_numbers: bool,
    is_err: bool,
}

/// A mutable view adapter for pane navigation.
///
/// Operations on this adapter mutate the underlying preview pane immediately.
pub struct Viewport<'a> {
    pane: &'a mut Pane,
    size: Dimensions,
}

impl From<Buffer> for Pane {
    fn from(buffer: Buffer) -> Self {
        Self::from_buffer(buffer)
    }
}

impl Pane {
    /// Initialize an empty pane with no offset, line numbers disabled, and the error flag set to
    /// `false`.
    ///
    /// This is the same as the [`Default`] implementation, but as a `const fn`.
    pub const fn new() -> Self {
        Self::from_buffer(Buffer::new())
    }

    pub(super) const fn from_buffer(buffer: Buffer) -> Self {
        Self {
            buffer,
            offset: (0, 0),
            line_numbers: false,
            is_err: false,
        }
    }

    /// Get access to the internal content buffer.
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// Get mutable access to the internal content buffer.
    ///
    /// Note that editing or clearing the buffer preserves the remaining pane state, including the
    /// error status and offset.
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    /// Convert into the internal buffer, discarding additional state.
    pub fn into_buffer(self) -> Buffer {
        self.buffer
    }

    /// Clear the buffer and restore all associated state to the default.
    ///
    /// The underlying allocation is retained for reuse.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.offset = (0, 0);
        self.line_numbers = false;
        self.is_err = false;
    }

    /// Append unstyled text and process newlines and control characters automatically.
    ///
    /// This forwards directly to [`Buffer::push_text`]. Read those docs for more.
    #[inline]
    pub fn push_text(&mut self, text: &str) {
        self.buffer.push_text(text);
    }

    /// Append styled text and process newlines and control characters automatically.
    ///
    /// This forwards directly to [`Buffer::push_styled_text`]. Read those docs for more.
    #[inline]
    pub fn push_styled_text(&mut self, text: &str, style: Style) {
        self.buffer.push_styled_text(text, style);
    }

    /// Returns a [`fmt::Write`](std::fmt::Write) adapter which defers writes to [`push_text`](Self::push_text).
    ///
    /// This forwards directly to [`Buffer::text_writer`]. Read those docs for more.
    #[inline]
    pub fn text_writer(&mut self) -> TextWriter<'_> {
        self.buffer.text_writer()
    }

    /// Write a stream of bytes with ANSI-encoded styling directly into this buffer.
    ///
    /// This forwards directly to [`Buffer::ansi_writer`]. Read those docs for more.
    #[cfg(feature = "preview-ansi")]
    #[cfg_attr(docsrs, doc(cfg(feature = "preview-ansi")))]
    #[inline]
    pub fn ansi_writer(&mut self) -> AnsiWriter<'_> {
        self.buffer.ansi_writer()
    }

    /// Set the failure status of the pane.
    #[inline]
    pub fn set_err(&mut self, is_err: bool) {
        self.is_err = is_err;
    }

    /// Get the failure status of the pane.
    #[inline]
    pub fn is_err(&self) -> bool {
        self.is_err
    }

    /// Returns the current viewport column and row offset.
    ///
    /// Both offsets are zero-based. The column refers to terminal display cells.
    pub fn offset(&self) -> (usize, usize) {
        self.offset
    }

    /// Set the viewport offset.
    ///
    /// This sets the coordinates of the top-left position of the preview buffer. The column
    /// refers to the horizontal offset in terms of terminal display cells.
    ///
    /// This method does not automatically clamp the offset, so the offset may correspond to a
    /// position which does not contain any preview buffer contents.
    pub fn set_offset(&mut self, column: usize, row: usize) {
        self.offset = (column, row);
    }

    /// Set the offset manually.
    ///
    /// Note that the provided offset is not clamped to be in range. The caller should check that the offset
    /// is in range or the preview may render in a strange way.
    #[must_use]
    pub fn with_offset(mut self, column: usize, row: usize) -> Self {
        self.set_offset(column, row);
        self
    }

    /// Returns a mutable viewport used for manipulating the preview pane.
    ///
    /// The provided dimensions should exclude the borders but include the space for the line-number
    /// gutter (which is dynamically set by the pane anyway). Width and height zero are permitted.
    ///
    /// This method exists since preview navigation depends on the size of the viewport. For
    /// instance, the preview pane will not scroll beyond the bottom of the page or beyond the end
    /// of the longest visible line. These computations depend on knowledge of the dimensions.
    ///
    /// # Example
    ///
    /// Here is an example demonstrating basic pane navigation.
    ///
    /// ```
    /// use nucleo_picker_core::{preview::Pane, session::Dimensions};
    ///
    /// let mut pane = Pane::new();
    /// pane.push_text("one\ntwo\nthree\nfour\nfive");
    ///
    /// // wide but very narrow
    /// let size = Dimensions { width: 20, height: 2 };
    ///
    /// // scroll down a whole page
    /// pane.viewport(size).page_down(1);
    /// assert_eq!(pane.offset(), (0, 2));
    ///
    /// // align to the bottom, which scrolls down a bit more
    /// pane.viewport(size).align_bottom();
    /// assert_eq!(pane.offset(), (0, 3));
    ///
    /// // when the size becomes larger, clamping aligns to
    /// // the bottom of the frame which moves the preview pane
    /// // origin up
    /// pane.viewport(Dimensions { height: 4, ..size }).clamp();
    /// assert_eq!(pane.offset(), (0, 1));
    /// ```
    #[must_use]
    pub fn viewport(&mut self, size: Dimensions) -> Viewport<'_> {
        Viewport { pane: self, size }
    }

    /// Enable or disable preview pane line-numbers.
    ///
    /// The default is `false` (do not display line numbers). Line numbers are displayed in a column
    /// on the left side of the preview pane and are automatically hidden if the preview pane has less
    /// than two columns for content.
    pub fn set_line_numbers(&mut self, enabled: bool) {
        self.line_numbers = enabled;
    }

    /// Toggle line-number visibility.
    pub fn toggle_line_numbers(&mut self) {
        self.line_numbers.toggle();
    }

    /// Returns the line-number display setting for this pane.
    pub fn line_numbers(&self) -> bool {
        self.line_numbers
    }

    /// A consuming equivalent of [`set_line_numbers`](Self::set_line_numbers).
    #[must_use]
    pub fn with_line_numbers(mut self, enabled: bool) -> Self {
        self.set_line_numbers(enabled);
        self
    }

    pub(super) fn visible_rows(&self, height: u16) -> Range<usize> {
        let height = usize::from(height);
        let lines = self.buffer().lines().len();
        let (_, row) = self.offset();
        let start = row.min(lines.saturating_sub(height));
        start..start.saturating_add(height).min(lines)
    }

    pub(super) fn number_width(&self, width: u16) -> u16 {
        if !self.line_numbers() {
            return 0;
        }
        let number_width = self.buffer().lines().len().ilog10() as u16 + 2;
        if number_width < width.saturating_sub(1) {
            number_width
        } else {
            0
        }
    }
}

impl Viewport<'_> {
    /// Scroll the visible row upwards, saturating at row zero and preserving the column position.
    pub fn up(&mut self, lines: usize) -> &mut Self {
        self.clamp_vertical();
        self.pane.offset.1 = self.pane.offset.1.saturating_sub(lines);
        self
    }

    /// Scroll the visible row downwards, saturating at the end of the preview buffer and preserving the column position.
    ///
    /// This will scroll downwards until the bottom of the buffer hits the bottom of the viewport.
    /// In particular, if the buffer is sufficiently large, there will be no blank lines remaining
    /// at the bottom of the rendered pane.
    pub fn down(&mut self, lines: usize) -> &mut Self {
        self.pane.offset.1 = self
            .pane
            .offset
            .1
            .saturating_add(lines)
            .min(self.vertical_limit());
        self
    }

    /// Srcoll the viewport left by `columns` many display cells, saturating at column 0.
    ///
    /// The internal row is unchanged.
    pub fn left(&mut self, columns: usize) -> &mut Self {
        self.pane.offset.0 = self.pane.offset.0.saturating_sub(columns);
        self
    }

    /// Scroll the viewport right by `columns` many display cells, saturating at the longest
    /// visible row.
    ///
    /// The scroll will be saturated so that the longest visible row terminates at the right end
    /// of the viewport. If the viewport is already past the longest visible row, this will do
    /// nothing.
    pub fn right(&mut self, columns: usize) -> &mut Self {
        let column = self.pane.offset.0;
        let target = column.saturating_add(columns);
        if target != column {
            let limit = self.horizontal_limit::<UnicodeProcessor>(target);
            // Vertical scrolling and layout changes can leave the offset past the local limit.
            self.pane.offset.0 = column.max(limit);
        }
        self
    }

    /// Scroll the viewport up by a `pages` many frames with the same saturation logic as `up`.
    pub fn page_up(&mut self, pages: usize) -> &mut Self {
        let lines = pages.saturating_mul(usize::from(self.size.height));
        self.up(lines)
    }

    /// Scroll the viewport down by a `pages` many frames with the same saturation logic as `down`.
    pub fn page_down(&mut self, pages: usize) -> &mut Self {
        let lines = pages.saturating_mul(usize::from(self.size.height));
        self.down(lines)
    }

    /// Align the viewport to row zero, preserving the column.
    pub fn align_top(&mut self) -> &mut Self {
        self.pane.offset.1 = 0;
        self
    }

    /// Align the bottom of the viewport to the final row, preserving the column.
    ///
    /// If the viewport is large, the buffer will remain at row zero (so there will be blank lines
    /// at the bottom).
    pub fn align_bottom(&mut self) -> &mut Self {
        self.pane.offset.1 = self.vertical_limit();
        self
    }

    /// Align the viewport to the left.
    pub fn align_left(&mut self) -> &mut Self {
        self.pane.offset.0 = 0;
        self
    }

    /// Align the right edge of the longest line to the right edge of the viewport.
    ///
    /// In particular, unlike [`right`](Self::right), this may cause the viewport to actually move
    /// left.
    pub fn align_right(&mut self) -> &mut Self {
        self.pane.offset.0 = self.horizontal_limit::<UnicodeProcessor>(usize::MAX);
        self
    }

    /// Clamp the row offset, preserving the column.
    ///
    /// Similarly to [`align_bottom`](Self::align_bottom), if the viewport has more lines than the buffer, there will
    /// be trailing blank lines.
    pub fn clamp_vertical(&mut self) -> &mut Self {
        self.pane.offset.1 = self.pane.offset.1.min(self.vertical_limit());
        self
    }

    /// Clamp the row offset followed by the column offset.
    pub fn clamp(&mut self) -> &mut Self {
        self.clamp_vertical();
        let column = self.pane.offset.0;
        if column != 0 {
            self.pane.offset.0 = self.horizontal_limit::<UnicodeProcessor>(column);
        }
        self
    }

    fn vertical_limit(&self) -> usize {
        self.pane
            .buffer()
            .lines()
            .len()
            .saturating_sub(usize::from(self.size.height))
    }

    fn horizontal_limit<P: Processor>(&self, target: usize) -> usize {
        let width = usize::from(self.size.width - self.pane.number_width(self.size.width));
        let measure_up_to = target.saturating_add(width);
        let mut maximum = 0;
        for index in self.pane.visible_rows(self.size.height) {
            let line = self.pane.buffer().line(index).unwrap();
            let measured = P::width_up_to(line.as_str(), measure_up_to);
            maximum = maximum.max(measured.saturating_sub(width));
            if maximum >= target {
                break;
            }
        }
        maximum.min(target)
    }
}

#[cfg(test)]
mod tests;
