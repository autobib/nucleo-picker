use std::{io, num::NonZero};

use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate},
};

use crate::{
    Picker, Render, Terminal,
    match_list::{MatchListStatus, Queued},
};

#[derive(Default, Clone, Copy)]
pub struct Redraw {
    pub prompt: bool,
    pub match_list: bool,
    pub match_status: bool,
}

impl Redraw {
    pub fn is_required(self) -> bool {
        self.prompt || self.match_list || self.match_status
    }

    pub fn all() -> Self {
        Self {
            prompt: true,
            match_list: true,
            match_status: true,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Default)]
pub struct FrameState {
    frame: u64,
    matching: bool,
    injecting: bool,
    displayed_injecting: bool,
    status_marker: Option<char>,
    spinner_index: usize,
}

impl FrameState {
    pub fn advance(&mut self, background_frame_frequency: NonZero<usize>) -> bool {
        self.frame = self.frame.wrapping_add(1);
        self.frame
            .is_multiple_of(background_frame_frequency.get() as u64)
    }

    pub fn observe(&mut self, status: &MatchListStatus) {
        self.matching = status.matching;
        self.injecting = status.injecting;
    }

    pub fn update_marker(
        &mut self,
        spinner_chars: &'static [char],
        matching_indicator: char,
    ) -> bool {
        if self.injecting && self.displayed_injecting {
            if !spinner_chars.is_empty() {
                self.spinner_index = (self.spinner_index + 1) % spinner_chars.len();
            }
        } else {
            self.spinner_index = 0;
        }

        let marker = if self.injecting {
            spinner_chars.get(self.spinner_index).copied()
        } else {
            self.matching.then_some(matching_indicator)
        };
        let changed = self.status_marker != marker;
        self.status_marker = marker;
        self.displayed_injecting = self.injecting;
        changed
    }

    /// Render the frame, specifying which parts of the frame need to be re-drawn.
    #[inline]
    pub fn render_frame<T: Send + Sync + 'static, R: Render<T>, W: Terminal, Q: Queued>(
        &self,
        picker: &mut Picker<T, R>,
        writer: &mut W,
        redraw: Redraw,
        queued_items: &Q,
    ) -> io::Result<()> {
        let (width, height) = writer.size()?;

        let (prompt_row, match_list_row) = if picker.reversed {
            (0, 1)
        } else {
            (height - 1, 0)
        };

        if width >= 1 && redraw.is_required() {
            writer.queue(BeginSynchronizedUpdate)?;

            if redraw.match_list && height >= 2 {
                writer.queue(MoveTo(0, match_list_row))?;

                picker.match_list.draw(
                    width,
                    height - 1,
                    writer,
                    |idx| queued_items.is_queued(idx),
                    queued_items.count(picker.max_selection_count),
                    self.status_marker,
                )?;
            } else if redraw.match_status && height >= 2 {
                let status_row = if picker.reversed {
                    match_list_row
                } else {
                    height - 2
                };
                writer.queue(MoveTo(0, status_row))?;
                picker.match_list.draw_status(
                    width,
                    writer,
                    queued_items.count(picker.max_selection_count),
                    self.status_marker,
                )?;
            }

            if redraw.prompt && height >= 1 {
                writer.queue(MoveTo(0, prompt_row))?;

                picker.prompt.draw(width, 1, writer)?;
            }

            writer
                .queue(MoveTo(picker.prompt.screen_offset() + 2, prompt_row))?
                .queue(EndSynchronizedUpdate)?;

            writer.flush()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameState, MatchListStatus};

    #[test]
    fn marker_state_is_local_to_a_pick() {
        let mut frame = FrameState::default();
        frame.observe(&MatchListStatus {
            items_changed: false,
            matching: true,
            injecting: true,
        });

        assert!(frame.update_marker(&['a', 'b'], 'm'));
        assert_eq!(frame.status_marker, Some('a'));
        assert!(frame.update_marker(&['a', 'b'], 'm'));
        assert_eq!(frame.status_marker, Some('b'));

        frame.observe(&MatchListStatus {
            items_changed: false,
            matching: true,
            injecting: false,
        });
        assert!(frame.update_marker(&['a', 'b'], 'm'));
        assert_eq!(frame.status_marker, Some('m'));

        assert_eq!(FrameState::default().status_marker, None);
    }
}
