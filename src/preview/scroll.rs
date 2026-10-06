use super::cache::{Cached, RequestState};

/// Events which target the preview pane.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewEvent {
    /// Scroll towards the beginning of the buffer by 1 line.
    Up(usize),
    /// Scroll towards the end of the buffer by 1 line.
    Down(usize),
    /// Scroll towards the beginning of the buffer by 1 page.
    PageUp(usize),
    /// Scroll towards the end of the buffer by 1 page.
    PageDown(usize),
    /// Toggle the presence of line numbers in an active preview pane.
    ToggleLineNumbers,
    /// Set visibility of line numbers in an active preview pane, or None to use the preview pane
    /// default.
    ///
    /// This event has no default keybind.
    SetLineNumbers(Option<bool>),
}

impl Cached {
    pub fn scroll(&mut self, event: PreviewEvent, height: u16) -> bool {
        if height == 0 {
            return false;
        }
        let Some(RequestState::Ready(buffer)) = &self.state else {
            return false;
        };
        let height = usize::from(height);
        let maximum = buffer.lines().len().saturating_sub(height);
        let position = self.scroll_position.min(maximum);
        let new_position = match event {
            PreviewEvent::Up(lines) => position.saturating_sub(lines),
            PreviewEvent::Down(lines) => position.saturating_add(lines).min(maximum),
            PreviewEvent::PageUp(pages) => position.saturating_sub(pages.saturating_mul(height)),
            PreviewEvent::PageDown(pages) => position
                .saturating_add(pages.saturating_mul(height))
                .min(maximum),
            PreviewEvent::ToggleLineNumbers | PreviewEvent::SetLineNumbers(_) => return false,
        };
        let changed = self.scroll_position != new_position;
        self.scroll_position = new_position;
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
            line_numbers_override: None,
        }
    }

    #[test]
    fn lines_and_pages_clamp_without_overflow() {
        let mut cached = ready(23);
        assert!(!cached.scroll(PreviewEvent::Up(1), 8));
        assert!(cached.scroll(PreviewEvent::Down(2), 8));
        assert_eq!(cached.scroll_position, 2);
        assert!(cached.scroll(PreviewEvent::PageDown(1), 8));
        assert_eq!(cached.scroll_position, 10);
        assert!(cached.scroll(PreviewEvent::PageDown(usize::MAX), 8));
        assert_eq!(cached.scroll_position, 15);
        assert!(!cached.scroll(PreviewEvent::Down(usize::MAX), 8));
        assert!(cached.scroll(PreviewEvent::PageUp(1), 8));
        assert_eq!(cached.scroll_position, 7);
        assert!(cached.scroll(PreviewEvent::Up(usize::MAX), 8));
        assert_eq!(cached.scroll_position, 0);
        assert!(cached.scroll(PreviewEvent::Down(usize::MAX), 8));
        assert_eq!(cached.scroll_position, 15);
        assert!(cached.scroll(PreviewEvent::PageUp(usize::MAX), 8));
        assert_eq!(cached.scroll_position, 0);
        assert!(!cached.scroll(PreviewEvent::Down(0), 8));
        assert!(!cached.scroll(PreviewEvent::PageDown(0), 8));
    }

    #[test]
    fn opposite_scroll_events_are_applied_in_order_at_boundaries() {
        let mut cached = ready(23);
        cached.scroll(PreviewEvent::Up(5), 8);
        cached.scroll(PreviewEvent::Down(5), 8);
        assert_eq!(cached.scroll_position, 5);
        cached.scroll(PreviewEvent::Down(usize::MAX), 8);
        cached.scroll(PreviewEvent::Down(5), 8);
        cached.scroll(PreviewEvent::Up(5), 8);
        assert_eq!(cached.scroll_position, 10);
    }

    #[test]
    fn short_and_empty_buffers_do_not_scroll() {
        for lines in [1, 3, 8] {
            let mut cached = ready(lines);
            assert!(!cached.scroll(PreviewEvent::Down(1), 8));
            assert!(!cached.scroll(PreviewEvent::PageDown(usize::MAX), 8));
            assert_eq!(cached.scroll_position, 0);
        }
    }

    #[test]
    fn zero_height_ignores_scroll_and_preserves_the_offset() {
        let mut cached = ready(23);
        cached.scroll_position = 9;
        for event in [
            PreviewEvent::Up(1),
            PreviewEvent::Down(1),
            PreviewEvent::PageUp(1),
            PreviewEvent::PageDown(1),
        ] {
            assert!(!cached.scroll(event, 0));
            assert_eq!(cached.scroll_position, 9);
        }
        cached.resize(0);
        assert_eq!(cached.scroll_position, 9);
    }

    #[test]
    fn resizing_clamps_the_offset_to_avoid_unused_space() {
        let mut cached = ready(23);
        cached.scroll(PreviewEvent::Down(usize::MAX), 8);
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
            line_numbers_override: None,
        };
        assert!(!cached.scroll(PreviewEvent::Down(1), 8));
        let active = queued.start().unwrap();
        assert!(!cached.scroll(PreviewEvent::PageDown(1), 8));
        let mut buffer = PreviewBuffer::new();
        assert!(active.publish(&mut buffer));
        assert!(!cached.scroll(PreviewEvent::PageUp(1), 8));
        cached.resize(8);
        assert_eq!(cached.scroll_position, 0);
        assert!(matches!(cached.state, Some(RequestState::Pending(_))));
    }
}
