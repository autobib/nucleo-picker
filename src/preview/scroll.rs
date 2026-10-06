use std::ops::Range;

use super::{
    PreviewBuffer,
    cache::{Cached, RequestState},
};
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
    /// the final visible row. If want to align to the right *without any possible left movement*,
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
    /// This event has no default keybind.
    AlignTop,
    /// Align the preview window to the final row, preserving the horizontal offset.
    ///
    /// This event has no default keybind.
    AlignBottom,
    /// Toggle the presence of line numbers in an active preview pane.
    ToggleLineNumbers,
    /// Set visibility of line numbers in an active preview pane, or None to use the preview pane
    /// default.
    ///
    /// This event has no default keybind.
    SetLineNumbers(Option<bool>),
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

impl Cached {
    pub(super) fn scroll(
        &mut self,
        movement: PreviewMovement,
        (width, height): (u16, u16),
    ) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let Some(RequestState::Ready(buffer)) = &self.state else {
            return false;
        };
        let height = usize::from(height);
        let maximum = buffer.lines().len().saturating_sub(height);
        let position = self.scroll_position.min(maximum);
        let horizontal = self.horizontal_position;
        let (offset, new_position) = match movement {
            PreviewMovement::AlignLeft => (&mut self.horizontal_position, 0),
            PreviewMovement::AlignRight => {
                let rows = position..position.saturating_add(height).min(buffer.lines().len());
                let limit = horizontal_limit::<UnicodeProcessor>(
                    buffer,
                    rows,
                    usize::MAX,
                    width - self.number_width(width),
                );
                (&mut self.horizontal_position, limit)
            }
            PreviewMovement::AlignTop => (&mut self.scroll_position, 0),
            PreviewMovement::AlignBottom => (&mut self.scroll_position, maximum),
            PreviewMovement::Left(columns) => (
                &mut self.horizontal_position,
                horizontal.saturating_sub(columns),
            ),
            PreviewMovement::Right(columns) => {
                let target = horizontal.saturating_add(columns);
                if target == horizontal {
                    return false;
                }
                let rows = position..position.saturating_add(height).min(buffer.lines().len());
                let limit = horizontal_limit::<UnicodeProcessor>(
                    buffer,
                    rows,
                    target,
                    width - self.number_width(width),
                );
                // Vertical scrolling and layout changes can leave the offset past the local limit.
                (&mut self.horizontal_position, horizontal.max(limit))
            }
            PreviewMovement::Up(lines) => {
                (&mut self.scroll_position, position.saturating_sub(lines))
            }
            PreviewMovement::Down(lines) => (
                &mut self.scroll_position,
                position.saturating_add(lines).min(maximum),
            ),
            PreviewMovement::PageUp(pages) => (
                &mut self.scroll_position,
                position.saturating_sub(pages.saturating_mul(height)),
            ),
            PreviewMovement::PageDown(pages) => (
                &mut self.scroll_position,
                position
                    .saturating_add(pages.saturating_mul(height))
                    .min(maximum),
            ),
        };
        let changed = *offset != new_position;
        *offset = new_position;
        changed
    }

    pub fn resize(&mut self, height: u16) {
        if height == 0 {
            return;
        }
        if let Some(RequestState::Ready(buffer)) = &self.state {
            self.scroll_position = self
                .scroll_position
                .min(buffer.lines().len().saturating_sub(usize::from(height)));
        }
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
    use crate::preview::{PreviewBuffer, lock, request::PendingPreview};

    fn ready(lines: usize) -> Cached {
        let mut buffer = PreviewBuffer::new();
        for _ in 1..lines {
            buffer.newline();
        }
        Cached {
            state: Some(RequestState::Ready(buffer)),
            scroll_position: 0,
            horizontal_position: 0,
            line_numbers_override: None,
        }
    }

    #[test]
    fn lines_and_pages_clamp_without_overflow() {
        let mut cached = ready(23);
        assert!(!cached.scroll(PreviewMovement::Up(1), (20, 8)));
        assert!(cached.scroll(PreviewMovement::Down(2), (20, 8)));
        assert_eq!(cached.scroll_position, 2);
        assert!(cached.scroll(PreviewMovement::PageDown(1), (20, 8)));
        assert_eq!(cached.scroll_position, 10);
        assert!(cached.scroll(PreviewMovement::PageDown(usize::MAX), (20, 8)));
        assert_eq!(cached.scroll_position, 15);
        assert!(!cached.scroll(PreviewMovement::Down(usize::MAX), (20, 8)));
        assert!(cached.scroll(PreviewMovement::PageUp(1), (20, 8)));
        assert_eq!(cached.scroll_position, 7);
        assert!(cached.scroll(PreviewMovement::Up(usize::MAX), (20, 8)));
        assert_eq!(cached.scroll_position, 0);
        assert!(cached.scroll(PreviewMovement::Down(usize::MAX), (20, 8)));
        assert_eq!(cached.scroll_position, 15);
        assert!(cached.scroll(PreviewMovement::PageUp(usize::MAX), (20, 8)));
        assert_eq!(cached.scroll_position, 0);
        assert!(!cached.scroll(PreviewMovement::Down(0), (20, 8)));
        assert!(!cached.scroll(PreviewMovement::PageDown(0), (20, 8)));
    }

    #[test]
    fn opposite_scroll_events_are_applied_in_order_at_boundaries() {
        let mut cached = ready(23);
        cached.scroll(PreviewMovement::Up(5), (20, 8));
        cached.scroll(PreviewMovement::Down(5), (20, 8));
        assert_eq!(cached.scroll_position, 5);
        cached.scroll(PreviewMovement::Down(usize::MAX), (20, 8));
        cached.scroll(PreviewMovement::Down(5), (20, 8));
        cached.scroll(PreviewMovement::Up(5), (20, 8));
        assert_eq!(cached.scroll_position, 10);
    }

    #[test]
    fn short_and_empty_buffers_do_not_scroll() {
        for lines in [1, 3, 8] {
            let mut cached = ready(lines);
            assert!(!cached.scroll(PreviewMovement::Down(1), (20, 8)));
            assert!(!cached.scroll(PreviewMovement::PageDown(usize::MAX), (20, 8)));
            assert!(!cached.scroll(PreviewMovement::AlignTop, (20, 8)));
            assert!(!cached.scroll(PreviewMovement::AlignBottom, (20, 8)));
            assert_eq!(cached.scroll_position, 0);
        }
    }

    #[test]
    fn zero_height_ignores_scroll_and_preserves_the_offset() {
        let mut cached = ready(23);
        cached.scroll_position = 9;
        for event in [
            PreviewMovement::Up(1),
            PreviewMovement::Down(1),
            PreviewMovement::PageUp(1),
            PreviewMovement::PageDown(1),
        ] {
            assert!(!cached.scroll(event, (20, 0)));
            assert_eq!(cached.scroll_position, 9);
        }
        cached.resize(0);
        assert_eq!(cached.scroll_position, 9);
    }

    #[test]
    fn resizing_clamps_the_offset_to_avoid_unused_space() {
        let mut cached = ready(23);
        cached.scroll(PreviewMovement::Down(usize::MAX), (20, 8));
        cached.resize(12);
        assert_eq!(cached.scroll_position, 11);
        cached.resize(4);
        assert_eq!(cached.scroll_position, 11);
        cached.resize(30);
        assert_eq!(cached.scroll_position, 0);
    }

    #[test]
    fn pending_previews_ignore_scrolling_even_after_publication() {
        let (reader, queued) = lock::request(PreviewBuffer::new());
        let pending = PendingPreview { reader, epoch: 0 };
        let mut cached = Cached {
            state: Some(RequestState::Pending(pending)),
            scroll_position: 0,
            horizontal_position: 0,
            line_numbers_override: None,
        };
        assert!(!cached.scroll(PreviewMovement::Down(1), (20, 8)));
        assert!(!cached.scroll(PreviewMovement::Right(1), (20, 8)));
        let active = queued.start().unwrap();
        assert!(!cached.scroll(PreviewMovement::PageDown(1), (20, 8)));
        let mut buffer = PreviewBuffer::new();
        assert!(active.publish(&mut buffer));
        assert!(!cached.scroll(PreviewMovement::Right(usize::MAX), (20, 8)));
        assert!(!cached.scroll(PreviewMovement::PageUp(1), (20, 8)));
        cached.scroll_position = 3;
        cached.horizontal_position = 9;
        for event in [
            PreviewMovement::AlignLeft,
            PreviewMovement::AlignRight,
            PreviewMovement::AlignTop,
            PreviewMovement::AlignBottom,
        ] {
            assert!(!cached.scroll(event, (20, 8)));
        }
        assert_eq!(cached.horizontal_position, 9);
        cached.resize(8);
        assert_eq!(cached.scroll_position, 3);
        assert!(matches!(cached.state, Some(RequestState::Pending(_))));
    }

    fn text_preview(text: &str) -> Cached {
        let mut cached = ready(1);
        let Some(RequestState::Ready(buffer)) = &mut cached.state else {
            unreachable!();
        };
        buffer.push_text(text);
        cached
    }

    #[test]
    fn vertical_alignment_preserves_columns_and_only_redraws_on_changes() {
        for height in [1, 8, 23, 40] {
            let mut cached = ready(23);
            cached.scroll_position = 3;
            cached.horizontal_position = 9;
            let bottom = 23_usize.saturating_sub(usize::from(height));
            assert!(cached.scroll(PreviewMovement::AlignBottom, (20, height)));
            assert_eq!(cached.scroll_position, bottom);
            assert!(!cached.scroll(PreviewMovement::AlignBottom, (20, height)));
            assert_eq!(cached.horizontal_position, 9);
            assert_eq!(
                cached.scroll(PreviewMovement::AlignTop, (20, height)),
                bottom != 0
            );
            assert_eq!(cached.scroll_position, 0);
            assert!(!cached.scroll(PreviewMovement::AlignTop, (20, height)));
            assert_eq!(cached.horizontal_position, 9);
        }
    }

    #[test]
    fn horizontal_alignment_uses_visible_columns_and_can_move_left() {
        let mut cached = text_preview(
            "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\nx\noffscreen and much longer",
        );
        cached.scroll_position = 1;
        cached.horizontal_position = usize::MAX;
        assert!(cached.scroll(PreviewMovement::AlignRight, (5, 2)));
        assert_eq!((cached.horizontal_position, cached.scroll_position), (3, 1));
        assert!(!cached.scroll(PreviewMovement::AlignRight, (5, 2)));
        assert!(cached.scroll(PreviewMovement::AlignLeft, (5, 2)));
        assert_eq!((cached.horizontal_position, cached.scroll_position), (0, 1));
        assert!(!cached.scroll(PreviewMovement::AlignLeft, (5, 2)));

        cached.line_numbers_override = Some(true);
        assert!(cached.scroll(PreviewMovement::AlignRight, (5, 2)));
        assert_eq!(cached.horizontal_position, 5);
        cached.line_numbers_override = Some(false);
        assert!(cached.scroll(PreviewMovement::AlignRight, (5, 2)));
        assert_eq!(cached.horizontal_position, 3);
        assert!(cached.scroll(PreviewMovement::AlignRight, (12, 2)));
        assert_eq!(cached.horizontal_position, 0);
        assert!(cached.scroll(PreviewMovement::AlignRight, (5, 2)));
        assert!(cached.scroll(PreviewMovement::Down(1), (5, 1)));
        assert_eq!(cached.horizontal_position, 3);
        assert!(cached.scroll(PreviewMovement::AlignRight, (5, 1)));
        assert_eq!((cached.horizontal_position, cached.scroll_position), (0, 2));
    }

    #[test]
    fn alignment_ignores_zero_sized_panes() {
        let mut cached = text_preview("abcdefghijklmnop\nx\n\ny");
        cached.scroll_position = 2;
        cached.horizontal_position = 7;
        for dimensions in [(0, 1), (1, 0), (0, 0)] {
            for event in [
                PreviewMovement::AlignLeft,
                PreviewMovement::AlignRight,
                PreviewMovement::AlignTop,
                PreviewMovement::AlignBottom,
            ] {
                assert!(!cached.scroll(event, dimensions));
                assert_eq!((cached.horizontal_position, cached.scroll_position), (7, 2));
            }
        }
        cached.state = None;
        for event in [
            PreviewMovement::AlignLeft,
            PreviewMovement::AlignRight,
            PreviewMovement::AlignTop,
            PreviewMovement::AlignBottom,
        ] {
            assert!(!cached.scroll(event, (5, 1)));
            assert_eq!((cached.horizontal_position, cached.scroll_position), (7, 2));
        }
    }

    #[test]
    fn horizontal_bounds_use_columns_and_only_visible_rows() {
        let mut cached = text_preview(
            "offscreen and much longer\ne\u{301}界👩🏽‍💻abc\n\nabcdef\noffscreen and much longer",
        );
        cached.scroll_position = 1;
        assert!(!cached.scroll(PreviewMovement::Left(1), (5, 3)));
        assert!(cached.scroll(PreviewMovement::Right(1), (5, 3)));
        assert_eq!(cached.horizontal_position, 1);
        assert!(cached.scroll(PreviewMovement::Right(usize::MAX), (5, 3)));
        assert_eq!(cached.horizontal_position, 3);
        for columns in [0, 1, usize::MAX] {
            assert!(!cached.scroll(PreviewMovement::Right(columns), (5, 3)));
        }
        assert!(cached.scroll(PreviewMovement::Left(1), (5, 3)));
        assert_eq!(cached.horizontal_position, 2);
        assert!(cached.scroll(PreviewMovement::Left(usize::MAX), (5, 3)));
        assert_eq!(cached.horizontal_position, 0);
        assert!(!cached.scroll(PreviewMovement::Left(0), (5, 3)));

        cached.horizontal_position = usize::MAX - 1;
        assert!(!cached.scroll(PreviewMovement::Right(10), (5, 3)));
        assert_eq!(cached.horizontal_position, usize::MAX - 1);
        cached.horizontal_position = usize::MAX;
        assert!(!cached.scroll(PreviewMovement::Right(1), (5, 3)));
        assert!(cached.scroll(PreviewMovement::Left(usize::MAX), (5, 3)));
    }

    #[test]
    fn vertical_scroll_and_resize_preserve_horizontal_position() {
        let mut cached = text_preview("abcdefghijklmnop\nx\n\nabcdefghijklmnop");
        assert!(cached.scroll(PreviewMovement::Right(usize::MAX), (6, 1)));
        assert_eq!(cached.horizontal_position, 10);
        for event in [PreviewMovement::Down(1), PreviewMovement::PageDown(1)] {
            assert!(cached.scroll(event, (6, 1)));
            assert!(!cached.scroll(PreviewMovement::Right(1), (6, 1)));
            assert_eq!(cached.horizontal_position, 10);
        }
        assert!(cached.scroll(PreviewMovement::Left(1), (6, 1)));
        assert_eq!(cached.horizontal_position, 9);
        assert!(cached.scroll(PreviewMovement::PageUp(usize::MAX), (6, 1)));
        assert_eq!(cached.horizontal_position, 9);
        cached.resize(3);
        assert_eq!(cached.horizontal_position, 9);
        assert!(!cached.scroll(PreviewMovement::Right(1), (40, 3)));
        assert_eq!(cached.horizontal_position, 9);
        assert!(cached.scroll(PreviewMovement::Right(1), (5, 3)));
        assert_eq!(cached.horizontal_position, 10);
    }

    #[test]
    fn horizontal_scroll_respects_gutter_overrides_and_narrow_panes() {
        let mut cached = text_preview("abcdefghij");
        let Some(RequestState::Ready(buffer)) = &mut cached.state else {
            unreachable!();
        };
        buffer.set_line_numbers(true);
        assert_eq!(cached.number_width(6), 2);
        assert!(cached.scroll(PreviewMovement::Right(usize::MAX), (6, 1)));
        assert_eq!(cached.horizontal_position, 6);
        cached.line_numbers_override = Some(false);
        assert!(!cached.scroll(PreviewMovement::Right(1), (6, 1)));
        assert_eq!(cached.horizontal_position, 6);
        cached.line_numbers_override = None;
        assert!(cached.scroll(PreviewMovement::Left(usize::MAX), (6, 1)));
        for width in [0, 1, 2, 3] {
            assert_eq!(cached.number_width(width), 0);
        }
        assert!(cached.scroll(PreviewMovement::Right(usize::MAX), (1, 1)));
        assert_eq!(cached.horizontal_position, 9);
        for dimensions in [(0, 1), (1, 0), (0, 0)] {
            for event in [PreviewMovement::Left(1), PreviewMovement::Right(1)] {
                assert!(!cached.scroll(event, dimensions));
                assert_eq!(cached.horizontal_position, 9);
            }
        }
        for text in ["", "abc", "abcde"] {
            let mut cached = text_preview(text);
            assert!(!cached.scroll(PreviewMovement::Right(usize::MAX), (5, 1)));
            assert!(!cached.scroll(PreviewMovement::AlignRight, (5, 1)));
            assert!(!cached.scroll(PreviewMovement::AlignLeft, (5, 1)));
            assert_eq!(cached.horizontal_position, 0);
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
