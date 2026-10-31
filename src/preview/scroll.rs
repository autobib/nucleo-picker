use std::ops::Range;

use super::PreviewBuffer;
use crate::util::unicode::{Processor, UnicodeProcessor};

/// Events which target the preview pane.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewEvent {
    /// Refresh the selected preview.
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

    #[test]
    fn lines_and_pages_clamp_without_overflow() {
        let mut buffer = ready(23);
        assert!(!scroll(&mut buffer, PreviewMovement::Up(1), (20, 8)));
        assert!(scroll(&mut buffer, PreviewMovement::Down(2), (20, 8)));
        assert_eq!(buffer.offset(), (0, 2));
        assert!(scroll(&mut buffer, PreviewMovement::PageDown(1), (20, 8)));
        assert_eq!(buffer.offset(), (0, 10));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::PageDown(usize::MAX),
            (20, 8)
        ));
        assert_eq!(buffer.offset(), (0, 15));
        assert!(!scroll(
            &mut buffer,
            PreviewMovement::Down(usize::MAX),
            (20, 8)
        ));
        assert!(scroll(&mut buffer, PreviewMovement::PageUp(1), (20, 8)));
        assert_eq!(buffer.offset(), (0, 7));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Up(usize::MAX),
            (20, 8)
        ));
        assert_eq!(buffer.offset(), (0, 0));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Down(usize::MAX),
            (20, 8)
        ));
        assert_eq!(buffer.offset(), (0, 15));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::PageUp(usize::MAX),
            (20, 8)
        ));
        assert_eq!(buffer.offset(), (0, 0));
        assert!(!scroll(&mut buffer, PreviewMovement::Down(0), (20, 8)));
        assert!(!scroll(&mut buffer, PreviewMovement::PageDown(0), (20, 8)));
    }

    #[test]
    fn opposite_scroll_events_are_applied_in_order_at_boundaries() {
        let mut buffer = ready(23);
        scroll(&mut buffer, PreviewMovement::Up(5), (20, 8));
        scroll(&mut buffer, PreviewMovement::Down(5), (20, 8));
        assert_eq!(buffer.offset(), (0, 5));
        scroll(&mut buffer, PreviewMovement::Down(usize::MAX), (20, 8));
        scroll(&mut buffer, PreviewMovement::Down(5), (20, 8));
        scroll(&mut buffer, PreviewMovement::Up(5), (20, 8));
        assert_eq!(buffer.offset(), (0, 10));
    }

    #[test]
    fn short_and_empty_buffers_do_not_scroll() {
        for lines in [1, 3, 8] {
            let mut buffer = ready(lines);
            assert!(!scroll(&mut buffer, PreviewMovement::Down(1), (20, 8)));
            assert!(!scroll(
                &mut buffer,
                PreviewMovement::PageDown(usize::MAX),
                (20, 8)
            ));
            assert!(!scroll(&mut buffer, PreviewMovement::AlignTop, (20, 8)));
            assert!(!scroll(&mut buffer, PreviewMovement::AlignBottom, (20, 8)));
            assert_eq!(buffer.offset(), (0, 0));
        }
    }

    #[test]
    fn zero_height_ignores_scroll_and_preserves_the_offset() {
        let mut buffer = ready(23);
        buffer.set_offset(0, 9);
        for event in [
            PreviewMovement::Up(1),
            PreviewMovement::Down(1),
            PreviewMovement::PageUp(1),
            PreviewMovement::PageDown(1),
        ] {
            assert!(!scroll(&mut buffer, event, (20, 0)));
            assert_eq!(buffer.offset(), (0, 9));
        }
        resize(&mut buffer, 0);
        assert_eq!(buffer.offset(), (0, 9));
    }

    #[test]
    fn resizing_clamps_the_offset_to_avoid_unused_space() {
        let mut buffer = ready(23);
        scroll(&mut buffer, PreviewMovement::Down(usize::MAX), (20, 8));
        resize(&mut buffer, 12);
        assert_eq!(buffer.offset(), (0, 11));
        resize(&mut buffer, 4);
        assert_eq!(buffer.offset(), (0, 11));
        resize(&mut buffer, 30);
        assert_eq!(buffer.offset(), (0, 0));
    }

    fn text_preview(text: &str) -> PreviewBuffer {
        let mut buffer = ready(1);
        buffer.push_text(text);
        buffer
    }

    #[test]
    fn vertical_alignment_preserves_columns_and_only_redraws_on_changes() {
        for height in [1, 8, 23, 40] {
            let mut buffer = ready(23);
            buffer.set_offset(9, 3);
            let bottom = 23_usize.saturating_sub(usize::from(height));
            assert!(scroll(
                &mut buffer,
                PreviewMovement::AlignBottom,
                (20, height)
            ));
            assert_eq!(buffer.offset(), (9, bottom));
            assert!(!scroll(
                &mut buffer,
                PreviewMovement::AlignBottom,
                (20, height)
            ));
            assert_eq!(buffer.offset(), (9, bottom));
            assert_eq!(
                scroll(&mut buffer, PreviewMovement::AlignTop, (20, height)),
                bottom != 0
            );
            assert_eq!(buffer.offset(), (9, 0));
            assert!(!scroll(
                &mut buffer,
                PreviewMovement::AlignTop,
                (20, height)
            ));
            assert_eq!(buffer.offset(), (9, 0));
        }
    }

    #[test]
    fn horizontal_alignment_uses_visible_columns_and_can_move_left() {
        let mut buffer = text_preview(
            "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\nx\noffscreen and much longer",
        );
        buffer.set_offset(usize::MAX, 1);
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (5, 2)));
        assert_eq!(buffer.offset(), (3, 1));
        assert!(!scroll(&mut buffer, PreviewMovement::AlignRight, (5, 2)));
        assert!(scroll(&mut buffer, PreviewMovement::AlignLeft, (5, 2)));
        assert_eq!(buffer.offset(), (0, 1));
        assert!(!scroll(&mut buffer, PreviewMovement::AlignLeft, (5, 2)));

        buffer.set_line_numbers(true);
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (5, 2)));
        assert_eq!(buffer.offset(), (5, 1));
        buffer.set_line_numbers(false);
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (5, 2)));
        assert_eq!(buffer.offset(), (3, 1));
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (12, 2)));
        assert_eq!(buffer.offset(), (0, 1));
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (5, 2)));
        assert!(scroll(&mut buffer, PreviewMovement::Down(1), (5, 1)));
        assert_eq!(buffer.offset(), (3, 2));
        assert!(scroll(&mut buffer, PreviewMovement::AlignRight, (5, 1)));
        assert_eq!(buffer.offset(), (0, 2));
    }

    #[test]
    fn alignment_ignores_zero_sized_panes() {
        let mut buffer = text_preview("abcdefghijklmnop\nx\n\ny");
        buffer.set_offset(7, 2);
        for dimensions in [(0, 1), (1, 0), (0, 0)] {
            for event in [
                PreviewMovement::AlignLeft,
                PreviewMovement::AlignRight,
                PreviewMovement::AlignTop,
                PreviewMovement::AlignBottom,
            ] {
                assert!(!scroll(&mut buffer, event, dimensions));
                assert_eq!(buffer.offset(), (7, 2));
            }
        }
    }

    #[test]
    fn horizontal_bounds_use_columns_and_only_visible_rows() {
        let mut buffer = text_preview(
            "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\n\nabcdef\noffscreen and much longer",
        );
        buffer.set_offset(0, 1);
        assert!(!scroll(&mut buffer, PreviewMovement::Left(1), (5, 3)));
        assert!(scroll(&mut buffer, PreviewMovement::Right(1), (5, 3)));
        assert_eq!(buffer.offset(), (1, 1));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (5, 3)
        ));
        assert_eq!(buffer.offset(), (3, 1));
        for columns in [0, 1, usize::MAX] {
            assert!(!scroll(
                &mut buffer,
                PreviewMovement::Right(columns),
                (5, 3)
            ));
        }
        assert!(scroll(&mut buffer, PreviewMovement::Left(1), (5, 3)));
        assert_eq!(buffer.offset(), (2, 1));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Left(usize::MAX),
            (5, 3)
        ));
        assert_eq!(buffer.offset(), (0, 1));
        assert!(!scroll(&mut buffer, PreviewMovement::Left(0), (5, 3)));

        buffer.set_offset(usize::MAX - 1, 1);
        assert!(!scroll(&mut buffer, PreviewMovement::Right(10), (5, 3)));
        assert_eq!(buffer.offset(), (usize::MAX - 1, 1));
        buffer.set_offset(usize::MAX, 1);
        assert!(!scroll(&mut buffer, PreviewMovement::Right(1), (5, 3)));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Left(usize::MAX),
            (5, 3)
        ));
    }

    #[test]
    fn vertical_scroll_and_resize_preserve_horizontal_position() {
        let mut buffer = text_preview("abcdefghijklmnop\nx\n\nabcdefghijklmnop");
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (6, 1)
        ));
        assert_eq!(buffer.offset(), (10, 0));
        for event in [PreviewMovement::Down(1), PreviewMovement::PageDown(1)] {
            assert!(scroll(&mut buffer, event, (6, 1)));
            assert!(!scroll(&mut buffer, PreviewMovement::Right(1), (6, 1)));
            let (column, _) = buffer.offset();
            assert_eq!(column, 10);
        }
        assert!(scroll(&mut buffer, PreviewMovement::Left(1), (6, 1)));
        assert_eq!(buffer.offset(), (9, 2));
        assert!(scroll(
            &mut buffer,
            PreviewMovement::PageUp(usize::MAX),
            (6, 1)
        ));
        assert_eq!(buffer.offset(), (9, 0));
        resize(&mut buffer, 3);
        assert_eq!(buffer.offset(), (9, 0));
        assert!(!scroll(&mut buffer, PreviewMovement::Right(1), (40, 3)));
        assert_eq!(buffer.offset(), (9, 0));
        assert!(scroll(&mut buffer, PreviewMovement::Right(1), (5, 3)));
        assert_eq!(buffer.offset(), (10, 0));
    }

    #[test]
    fn horizontal_scroll_respects_line_numbers_and_narrow_panes() {
        let mut buffer = text_preview("abcdefghij");
        buffer.set_line_numbers(true);
        assert_eq!(number_width(&buffer, 6), 2);
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (6, 1)
        ));
        assert_eq!(buffer.offset(), (6, 0));
        buffer.set_line_numbers(false);
        assert!(!scroll(&mut buffer, PreviewMovement::Right(1), (6, 1)));
        assert_eq!(buffer.offset(), (6, 0));
        buffer.set_line_numbers(true);
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Left(usize::MAX),
            (6, 1)
        ));
        for width in [0, 1, 2, 3] {
            assert_eq!(number_width(&buffer, width), 0);
        }
        assert!(scroll(
            &mut buffer,
            PreviewMovement::Right(usize::MAX),
            (1, 1)
        ));
        assert_eq!(buffer.offset(), (9, 0));
        for dimensions in [(0, 1), (1, 0), (0, 0)] {
            for event in [PreviewMovement::Left(1), PreviewMovement::Right(1)] {
                assert!(!scroll(&mut buffer, event, dimensions));
                assert_eq!(buffer.offset(), (9, 0));
            }
        }
        for text in ["", "abc", "abcde"] {
            let mut buffer = text_preview(text);
            assert!(!scroll(
                &mut buffer,
                PreviewMovement::Right(usize::MAX),
                (5, 1)
            ));
            assert!(!scroll(&mut buffer, PreviewMovement::AlignRight, (5, 1)));
            assert!(!scroll(&mut buffer, PreviewMovement::AlignLeft, (5, 1)));
            assert_eq!(buffer.offset(), (0, 0));
        }
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
