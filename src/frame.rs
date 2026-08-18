use std::{io, io::Write, num::NonZero};

use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    terminal::{BeginSynchronizedUpdate, EndSynchronizedUpdate, size},
};

use crate::{
    Picker, Render,
    match_list::{MatchListStatus, Queued},
};

#[derive(Default)]
pub struct FrameState {
    frame: u64,
    width: u16,
    height: u16,
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
    pub fn render_frame<T: Send + Sync + 'static, R: Render<T>, W: Write, Q: Queued>(
        &mut self,
        picker: &mut Picker<T, R>,
        writer: &mut W,
        redraw_prompt: bool,
        redraw_match_list: bool,
        redraw_match_status: bool,
        queued_items: &Q,
    ) -> io::Result<()> {
        let (width, height) = size()?;
        self.width = width;
        self.height = height;

        let (prompt_row, match_list_row) = if picker.reversed {
            (0, 1)
        } else {
            (height - 1, 0)
        };

        if width >= 1 && (redraw_prompt || redraw_match_list || redraw_match_status) {
            #[cfg(feature = "tracing")]
            let _frame_entered = tracing::trace_span!(
                target: "nucleo_picker::frame",
                "picker.frame.render",
                sequence = self.frame,
                width,
                height,
                redraw_prompt,
                redraw_match_list,
                redraw_match_status,
            )
            .entered();
            writer.queue(BeginSynchronizedUpdate)?;

            if redraw_match_list && height >= 2 {
                writer.queue(MoveTo(0, match_list_row))?;

                #[cfg(feature = "tracing")]
                let (matched, total) = picker.match_list.trace_counts();
                #[cfg(feature = "tracing")]
                let _match_entered = tracing::trace_span!(
                    target: "nucleo_picker::frame",
                    "picker.frame.match_list",
                    status_only = false,
                    matched,
                    total,
                    visible_items = if matched == 0 { 0 } else { picker.match_list.selection_range().count() },
                )
                .entered();
                picker.match_list.draw(
                    width,
                    height - 1,
                    writer,
                    |idx| queued_items.is_queued(idx),
                    queued_items.count(picker.max_selection_count),
                    self.status_marker,
                )?;
            } else if redraw_match_status && height >= 2 {
                #[cfg(feature = "tracing")]
                let _match_entered = tracing::trace_span!(
                    target: "nucleo_picker::frame",
                    "picker.frame.match_list",
                    status_only = true,
                    matched = picker.match_list.trace_counts().0,
                    total = picker.match_list.trace_counts().1,
                    visible_items = 0,
                )
                .entered();
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

            if redraw_prompt && height >= 1 {
                #[cfg(feature = "tracing")]
                let _prompt_entered = tracing::trace_span!(
                    target: "nucleo_picker::frame",
                    "picker.frame.prompt",
                    query_bytes = picker.prompt.contents().len(),
                    query_chars = picker.prompt.contents().chars().count(),
                )
                .entered();
                writer.queue(MoveTo(0, prompt_row))?;

                picker.prompt.draw(width, 1, writer)?;
            }

            writer
                .queue(MoveTo(picker.prompt.screen_offset() + 2, prompt_row))?
                .queue(EndSynchronizedUpdate)?;

            {
                #[cfg(feature = "tracing")]
                let _flush_entered = tracing::trace_span!(
                    target: "nucleo_picker::frame",
                    "picker.frame.flush"
                )
                .entered();
                writer.flush()?;
            }
        }

        Ok(())
    }

    #[cfg(feature = "tracing")]
    pub fn trace_span(
        &self,
        selection_limit: Option<NonZero<u32>>,
        reversed: bool,
    ) -> tracing::Span {
        tracing::span!(
            target: "nucleo_picker::frame",
            tracing::Level::DEBUG,
            "picker.frame",
            sequence = self.frame,
            width = tracing::field::Empty,
            height = tracing::field::Empty,
            query = tracing::field::Empty,
            matching = tracing::field::Empty,
            injecting = tracing::field::Empty,
            matched = tracing::field::Empty,
            total = tracing::field::Empty,
            selected_match = tracing::field::Empty,
            selected_input_index = tracing::field::Empty,
            queued = tracing::field::Empty,
            selection_limit = selection_limit.map(NonZero::get),
            reversed,
        )
    }

    #[cfg(feature = "tracing")]
    pub fn record_trace<T: Send + Sync + 'static, R: Render<T>, Q: Queued>(
        &self,
        span: &tracing::Span,
        picker: &Picker<T, R>,
        queued_items: &Q,
    ) {
        let (matched, total) = picker.match_list.trace_counts();
        let selected_match = (matched != 0).then_some(picker.match_list.selection());
        let selected_input_index = picker.match_list.trace_selected_input_index();

        span.record("width", self.width);
        span.record("height", self.height);
        span.record("query", picker.prompt.contents());
        span.record("matching", self.matching);
        span.record("injecting", self.injecting);
        span.record("matched", matched);
        span.record("total", total);
        span.record("selected_match", selected_match);
        span.record("selected_input_index", selected_input_index);
        span.record("queued", queued_items.len());
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

    #[cfg(feature = "tracing")]
    mod tracing;
}
