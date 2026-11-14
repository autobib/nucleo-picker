use std::ops::Range;

use super::PreviewBuffer;
use crate::util::unicode::{Processor, UnicodeProcessor};

/// Events which target the preview pane.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewEvent {
    /// Refresh the preview for the hovered item.
    Refresh,
    /// Scroll to the left by `n` columns.
    Left(usize),
    /// Scroll to the right by `n` columns.
    Right(usize),
    /// Align the preview window with the first column.
    AlignLeft,
    /// Align the right edge of the preview window with the longest currently visible row, or to
    /// the first column if all of the rows fit.
    ///
    /// Note that this may result in a left movement if the preview pane is currently scrolled past
    /// the final visible row. If you want to align to the right *without any possible left movement*,
    /// use `PreviewEvent::Right(usize::MAX)`.
    AlignRight,
    /// Scroll towards the beginning of the buffer by `n` lines.
    Up(usize),
    /// Scroll towards the end of the buffer by `n` lines.
    Down(usize),
    /// Scroll towards the beginning of the buffer by `n` pages.
    PageUp(usize),
    /// Scroll towards the end of the buffer by `n` pages.
    PageDown(usize),
    /// Align the preview window to the first row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignTop,
    /// Align the preview window to the final row, preserving the horizontal offset.
    ///
    /// This event has no default keybinding.
    AlignBottom,
    /// Toggle the presence of line numbers in an active preview pane.
    ToggleLineNumbers,
    /// Set visibility of line numbers in the active preview pane.
    ///
    /// This event has no default keybinding.
    SetLineNumbers(bool),
}

pub(super) enum PreviewMovement {
    Left(usize),
    Right(usize),
    AlignLeft,
    AlignRight,
    Up(usize),
    Down(usize),
    PageUp(usize),
    PageDown(usize),
    AlignTop,
    AlignBottom,
}

pub(super) fn scroll(
    buffer: &mut PreviewBuffer,
    movement: PreviewMovement,
    (width, height): (u16, u16),
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let rows = visible_rows(buffer, height);
    let height = usize::from(height);
    let maximum = buffer.lines().len().saturating_sub(height);
    let position = rows.start;
    let previous = buffer.offset();
    let (mut column, mut row) = previous;
    match movement {
        PreviewMovement::AlignLeft => column = 0,
        PreviewMovement::AlignRight => {
            column = horizontal_limit::<UnicodeProcessor>(
                buffer,
                rows,
                usize::MAX,
                width - number_width(buffer, width),
            );
        }
        PreviewMovement::AlignTop => row = 0,
        PreviewMovement::AlignBottom => row = maximum,
        PreviewMovement::Left(columns) => {
            column = column.saturating_sub(columns);
        }
        PreviewMovement::Right(columns) => {
            let target = column.saturating_add(columns);
            if target == column {
                return false;
            }
            let limit = horizontal_limit::<UnicodeProcessor>(
                buffer,
                rows,
                target,
                width - number_width(buffer, width),
            );
            // Vertical scrolling and layout changes can leave the offset past the local limit.
            column = column.max(limit);
        }
        PreviewMovement::Up(lines) => row = position.saturating_sub(lines),
        PreviewMovement::Down(lines) => {
            row = position.saturating_add(lines).min(maximum);
        }
        PreviewMovement::PageUp(pages) => {
            row = position.saturating_sub(pages.saturating_mul(height));
        }
        PreviewMovement::PageDown(pages) => {
            row = position
                .saturating_add(pages.saturating_mul(height))
                .min(maximum);
        }
    }
    buffer.set_offset(column, row);
    (column, row) != previous
}

pub(super) fn resize(buffer: &mut PreviewBuffer, height: u16) {
    if height != 0 {
        let (column, _) = buffer.offset();
        buffer.set_offset(column, visible_rows(buffer, height).start);
    }
}

pub(super) fn visible_rows(buffer: &PreviewBuffer, height: u16) -> Range<usize> {
    let height = usize::from(height);
    let lines = buffer.lines().len();
    let (_, row) = buffer.offset();
    let start = row.min(lines.saturating_sub(height));
    start..start.saturating_add(height).min(lines)
}

pub(super) fn number_width(buffer: &PreviewBuffer, width: u16) -> u16 {
    if !buffer.line_numbers() {
        return 0;
    }
    let number_width = buffer.lines().len().ilog10() as u16 + 2;
    if number_width < width.saturating_sub(1) {
        number_width
    } else {
        0
    }
}

fn horizontal_limit<P: Processor>(
    buffer: &PreviewBuffer,
    rows: Range<usize>,
    target: usize,
    width: u16,
) -> usize {
    let width = usize::from(width);
    let measure_up_to = target.saturating_add(width);
    let mut maximum = 0;
    for index in rows {
        let line = buffer.line(index).unwrap();
        let measured = P::width_up_to(line.as_str(), measure_up_to);
        maximum = maximum.max(measured.saturating_sub(width));
        if maximum >= target {
            break;
        }
    }
    maximum.min(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(lines: usize) -> PreviewBuffer {
        let mut buffer = PreviewBuffer::new();
        for _ in 1..lines {
            buffer.newline();
        }
        buffer
    }

    fn assert_scroll(
        buffer: &mut PreviewBuffer,
        movement: PreviewMovement,
        dimensions: (u16, u16),
        expected: (usize, usize),
    ) {
        let previous = buffer.offset();
        assert_eq!(scroll(buffer, movement, dimensions), previous != expected);
        assert_eq!(buffer.offset(), expected);
    }

    #[test]
    fn lines_and_pages_saturate_in_order_and_preserve_columns() {
        let mut buffer = ready(23);
        buffer.set_offset(9, 0);
        for (movement, row) in [
            (PreviewMovement::Up(5), 0),
            (PreviewMovement::Down(2), 2),
            (PreviewMovement::PageDown(1), 10),
            (PreviewMovement::PageDown(usize::MAX), 15),
            (PreviewMovement::Down(usize::MAX), 15),
            (PreviewMovement::Down(5), 15),
            (PreviewMovement::Up(5), 10),
            (PreviewMovement::PageUp(1), 2),
            (PreviewMovement::Up(usize::MAX), 0),
            (PreviewMovement::Down(usize::MAX), 15),
            (PreviewMovement::PageUp(usize::MAX), 0),
            (PreviewMovement::Down(0), 0),
            (PreviewMovement::PageDown(0), 0),
        ] {
            assert_scroll(&mut buffer, movement, (20, 8), (9, row));
        }
        for lines in [1, 3, 8] {
            let mut buffer = ready(lines);
            for movement in [
                PreviewMovement::Down(1),
                PreviewMovement::PageDown(usize::MAX),
                PreviewMovement::AlignTop,
                PreviewMovement::AlignBottom,
            ] {
                assert_scroll(&mut buffer, movement, (20, 8), (0, 0));
            }
        }
    }

    fn text_preview(text: &str) -> PreviewBuffer {
        let mut buffer = ready(1);
        buffer.push_text(text);
        buffer
    }

    #[test]
    fn vertical_alignment_and_resize_clamp_rows_and_preserve_columns() {
        for height in [1, 8, 23, 40] {
            let mut buffer = ready(23);
            buffer.set_offset(9, 3);
            let bottom = 23_usize.saturating_sub(usize::from(height));
            for (movement, row) in [
                (PreviewMovement::AlignBottom, bottom),
                (PreviewMovement::AlignBottom, bottom),
                (PreviewMovement::AlignTop, 0),
                (PreviewMovement::AlignTop, 0),
            ] {
                assert_scroll(&mut buffer, movement, (20, height), (9, row));
            }
        }
        let mut buffer = ready(23);
        buffer.set_offset(9, usize::MAX);
        for (height, row) in [(8, 15), (12, 11), (4, 11), (30, 0)] {
            resize(&mut buffer, height);
            assert_eq!(buffer.offset(), (9, row));
        }
    }

    #[test]
    fn horizontal_bounds_and_alignment_use_visible_columns_and_line_numbers() {
        let mut buffer = text_preview(
            "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\n\nabcdef\noffscreen and much longer",
        );
        buffer.set_offset(0, 1);
        for (movement, column) in [
            (PreviewMovement::Left(1), 0),
            (PreviewMovement::Right(1), 1),
            (PreviewMovement::Right(usize::MAX), 3),
            (PreviewMovement::Right(0), 3),
            (PreviewMovement::Right(1), 3),
            (PreviewMovement::Right(usize::MAX), 3),
            (PreviewMovement::Left(1), 2),
            (PreviewMovement::Left(usize::MAX), 0),
            (PreviewMovement::Left(0), 0),
            (PreviewMovement::AlignRight, 3),
            (PreviewMovement::AlignRight, 3),
            (PreviewMovement::AlignLeft, 0),
            (PreviewMovement::AlignLeft, 0),
        ] {
            assert_scroll(&mut buffer, movement, (5, 3), (column, 1));
        }
        for column in [usize::MAX - 1, usize::MAX] {
            buffer.set_offset(column, 1);
            assert_scroll(&mut buffer, PreviewMovement::Right(10), (5, 3), (column, 1));
            assert_scroll(&mut buffer, PreviewMovement::AlignRight, (5, 3), (3, 1));
        }
        buffer.set_line_numbers(true);
        assert_eq!(number_width(&buffer, 6), 2);
        assert_scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (5, 3),
            (5, 1),
        );
        assert_scroll(&mut buffer, PreviewMovement::AlignRight, (5, 3), (5, 1));
        buffer.set_line_numbers(false);
        assert_scroll(&mut buffer, PreviewMovement::Right(1), (5, 3), (5, 1));
        assert_scroll(&mut buffer, PreviewMovement::AlignRight, (5, 3), (3, 1));
        assert_scroll(&mut buffer, PreviewMovement::AlignRight, (12, 3), (0, 1));
        assert_scroll(&mut buffer, PreviewMovement::AlignRight, (5, 3), (3, 1));
        assert_scroll(&mut buffer, PreviewMovement::Down(1), (5, 1), (3, 2));
        assert_scroll(&mut buffer, PreviewMovement::AlignRight, (5, 1), (0, 2));

        buffer.set_line_numbers(true);
        for width in [0, 1, 2, 3] {
            assert_eq!(number_width(&buffer, width), 0);
        }
        buffer.set_offset(0, 1);
        assert_scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (1, 3),
            (7, 1),
        );
        for text in ["", "abc", "abcde"] {
            let mut buffer = text_preview(text);
            for movement in [
                PreviewMovement::Right(usize::MAX),
                PreviewMovement::AlignRight,
                PreviewMovement::AlignLeft,
            ] {
                assert_scroll(&mut buffer, movement, (5, 1), (0, 0));
            }
        }
    }

    #[test]
    fn zero_sized_panes_preserve_offsets_for_all_movements() {
        let mut buffer = text_preview("abcdefghijklmnop\nx\n\ny");
        buffer.set_offset(7, 2);
        for dimensions in [(0, 1), (1, 0), (0, 0)] {
            for event in [
                PreviewMovement::Left(1),
                PreviewMovement::Right(1),
                PreviewMovement::Up(1),
                PreviewMovement::Down(1),
                PreviewMovement::PageUp(1),
                PreviewMovement::PageDown(1),
                PreviewMovement::AlignLeft,
                PreviewMovement::AlignRight,
                PreviewMovement::AlignTop,
                PreviewMovement::AlignBottom,
            ] {
                assert_scroll(&mut buffer, event, dimensions, (7, 2));
            }
        }
        resize(&mut buffer, 0);
        assert_eq!(buffer.offset(), (7, 2));
    }

    #[test]
    fn vertical_scroll_and_resize_preserve_horizontal_position() {
        let mut buffer = text_preview("abcdefghijklmnop\nx\n\nabcdefghijklmnop");
        assert_scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (6, 1),
            (10, 0),
        );
        for (movement, row) in [
            (PreviewMovement::Down(1), 1),
            (PreviewMovement::PageDown(1), 2),
        ] {
            assert_scroll(&mut buffer, movement, (6, 1), (10, row));
            assert_scroll(&mut buffer, PreviewMovement::Right(1), (6, 1), (10, row));
        }
        assert_scroll(&mut buffer, PreviewMovement::Left(1), (6, 1), (9, 2));
        assert_scroll(
            &mut buffer,
            PreviewMovement::PageUp(usize::MAX),
            (6, 1),
            (9, 0),
        );
        resize(&mut buffer, 3);
        assert_eq!(buffer.offset(), (9, 0));
        assert_scroll(&mut buffer, PreviewMovement::Right(1), (40, 3), (9, 0));
        assert_scroll(&mut buffer, PreviewMovement::Right(1), (5, 3), (10, 0));
    }

    #[test]
    fn horizontal_measurement_skips_offscreen_rows_and_hidden_suffixes() {
        use crate::util::unicode::tests::{CountingProcessor, MEASURED_BYTES};

        let mut buffer = PreviewBuffer::new();
        let long = "界".repeat(1_000_000);
        buffer.push_line(&long);
        buffer.push_line("abc");
        buffer.push_line("defghij");
        buffer.push_line(&long);
        buffer.push_str(&long);

        MEASURED_BYTES.set(0);
        assert_eq!(
            horizontal_limit::<CountingProcessor>(&buffer, 1..3, usize::MAX, 5),
            2
        );
        assert_eq!(MEASURED_BYTES.get(), 10);

        MEASURED_BYTES.set(0);
        assert_eq!(
            horizontal_limit::<CountingProcessor>(&buffer, 3..5, 1, 20),
            1
        );
        assert_eq!(MEASURED_BYTES.get(), 33);

        MEASURED_BYTES.set(0);
        assert_eq!(
            horizontal_limit::<CountingProcessor>(&buffer, 1..5, 1, 20),
            1
        );
        assert_eq!(MEASURED_BYTES.get(), 43);
    }
}
