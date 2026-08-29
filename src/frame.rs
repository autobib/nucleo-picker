use std::{io, num::NonZero};

use crossterm::{
    QueueableCommand,
    cursor::MoveTo,
    style::{Attribute, ResetColor, SetAttribute},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
};

use crate::{
    Picker, Render, Terminal,
    match_list::{MatchListStatus, Queued},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClearMode {
    All,
    Line,
    Exact,
}

#[derive(Default, Clone, Copy)]
pub struct Redraw {
    pub prompt: bool,
    pub match_list: bool,
    pub match_status: bool,
}

impl Redraw {
    pub fn any_required(self) -> bool {
        self.prompt || self.match_list || self.match_status
    }

    pub fn all_required(self) -> bool {
        self.prompt && self.match_list && self.match_status
    }

    pub fn all() -> Self {
        Self {
            prompt: true,
            match_list: true,
            match_status: true,
        }
    }

    pub fn set_all(&mut self) {
        *self = Self::all();
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScreenSize {
    width: u16,
    height: u16,
}

impl From<(u16, u16)> for ScreenSize {
    fn from((width, height): (u16, u16)) -> Self {
        Self { width, height }
    }
}

pub struct SizeChange {
    width: bool,
    height: bool,
}

impl SizeChange {
    pub fn is_changed(&self) -> bool {
        self.width || self.height
    }

    pub fn height_changed(&self) -> bool {
        self.height
    }
}

pub struct FrameState {
    size: ScreenSize,
    frame: u64,
    matching: bool,
    injecting: bool,
    displayed_injecting: bool,
    status_marker: Option<char>,
    spinner_index: usize,
}

impl FrameState {
    pub fn new(size: (u16, u16)) -> Self {
        Self {
            size: size.into(),
            frame: 0,
            matching: false,
            injecting: false,
            displayed_injecting: false,
            status_marker: None,
            spinner_index: 0,
        }
    }

    pub fn update_size(&mut self, size: (u16, u16)) -> SizeChange {
        let size = ScreenSize::from(size);
        let change = SizeChange {
            width: self.size.width != size.width,
            height: self.size.height != size.height,
        };
        self.size = size;
        change
    }

    pub fn match_list_height(&self) -> u16 {
        self.size.height.saturating_sub(2)
    }

    pub fn dimensions(&self) -> (u16, u16) {
        (self.size.width, self.size.height)
    }

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

    /// Render the frame the parts of the frame that need to be re-drawn.
    ///
    /// This method should only be called if there are frame changes, since it will still emit
    /// syscalls even if there was nothing to write.
    #[inline]
    pub fn render_frame<T: Send + Sync + 'static, R: Render<T>, W: Terminal, Q: Queued>(
        &self,
        picker: &mut Picker<T, R>,
        writer: &mut W,
        redraw: Redraw,
        queued_items: &Q,
    ) -> io::Result<()> {
        let ScreenSize { width, height } = self.size;
        let clear_mode = if redraw.all_required() {
            ClearMode::All
        } else {
            ClearMode::Line
        };
        let (prompt_row, match_status_row, match_list_row) = if picker.reversed {
            (0, 1, 2)
        } else {
            (height.saturating_sub(1), height.saturating_sub(2), 0)
        };

        if width >= 1 {
            writer.begin_render()?;
            writer.queue(BeginSynchronizedUpdate)?;

            if clear_mode == ClearMode::All {
                writer
                    .queue(ResetColor)?
                    .queue(SetAttribute(Attribute::Reset))?
                    .queue(Clear(ClearType::All))?;
            }

            if redraw.match_list && height >= 3 {
                writer.queue(MoveTo(0, match_list_row))?;
                picker
                    .match_list
                    .draw_items(width, clear_mode, writer, |idx| queued_items.is_queued(idx))?;
            }

            if redraw.match_status && height >= 2 {
                writer.queue(MoveTo(0, match_status_row))?;
                picker.match_list.draw_status(
                    width,
                    clear_mode,
                    writer,
                    queued_items.count(picker.max_selection_count),
                    self.status_marker,
                )?;
            }

            if redraw.prompt && height >= 1 {
                writer.queue(MoveTo(0, prompt_row))?;

                picker.prompt.draw(width, 1, clear_mode, writer)?;
            }

            writer
                .queue(MoveTo(
                    picker
                        .prompt
                        .screen_offset()
                        .saturating_add(2)
                        .min(width - 1),
                    prompt_row,
                ))?
                .queue(EndSynchronizedUpdate)?;

            writer.end_render()?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use crate::{Picker, Terminal, render::StrRenderer};

    use super::{FrameState, MatchListStatus, Redraw};

    struct TestTerminal {
        output: Vec<u8>,
        size: (u16, u16),
    }

    impl Write for TestTerminal {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Terminal for TestTerminal {
        fn init(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn cleanup(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn size(&mut self) -> io::Result<(u16, u16)> {
            Ok(self.size)
        }
    }

    fn render(redraw: Redraw) -> String {
        let mut picker = Picker::<String, _>::new(StrRenderer);
        let mut terminal = TestTerminal {
            output: Vec::new(),
            size: (20, 4),
        };

        FrameState::new((20, 4))
            .render_frame(&mut picker, &mut terminal, redraw, &())
            .unwrap();

        String::from_utf8(terminal.output).unwrap()
    }

    #[test]
    fn match_list_redraw_does_not_draw_match_status() {
        let output = render(Redraw {
            match_list: true,
            ..Redraw::default()
        });

        assert!(!output.contains("0/0"));
    }

    #[test]
    fn match_status_can_be_redrawn_independently() {
        let output = render(Redraw {
            match_status: true,
            ..Redraw::default()
        });

        assert!(output.contains("0/0"));
    }

    #[test]
    fn full_redraw_uses_a_single_screen_clear_strategy() {
        let output = render(Redraw::all());

        assert!(output.contains("\x1b[2J"));
        assert!(!output.contains("\x1b[2K"));
    }

    #[test]
    fn marker_state_is_local_to_a_pick() {
        let mut frame = FrameState::new((20, 4));
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

        assert_eq!(FrameState::new((20, 4)).status_marker, None);
    }

    #[test]
    fn background_frames_follow_the_configured_frequency() {
        let mut frame = FrameState::new((20, 4));
        let frequency = std::num::NonZero::new(3).unwrap();

        assert!(!frame.advance(frequency));
        assert!(!frame.advance(frequency));
        assert!(frame.advance(frequency));
        assert!(!frame.advance(frequency));
    }

    #[test]
    fn screen_size_changes_track_dimensions_separately() {
        let mut frame = FrameState::new((20, 4));

        let width_change = frame.update_size((30, 4));
        assert!(width_change.is_changed());
        assert!(!width_change.height_changed());

        let height_change = frame.update_size((30, 8));
        assert!(height_change.is_changed());
        assert!(height_change.height_changed());
        assert_eq!(frame.match_list_height(), 6);

        let no_change = frame.update_size((30, 8));
        assert!(!no_change.is_changed());
        assert!(!no_change.height_changed());
    }
}
