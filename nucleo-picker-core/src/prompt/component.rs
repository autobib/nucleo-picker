use crate::draw::Style;

use super::{PromptEvent, PromptState, state::PromptStatus};
use crate::{
    PickerChars,
    component::Component,
    draw::Surface,
    rect::{Dimensions, Position},
};

pub(crate) struct Prompt<'a> {
    state: &'a mut PromptState,
    chars: &'a PickerChars,
    size: Option<Dimensions>,
    buffered_event: Option<PromptEvent>,
    pending: PromptStatus,
}

impl<'a> Prompt<'a> {
    pub fn new(state: &'a mut PromptState, chars: &'a PickerChars) -> Self {
        Self {
            state,
            chars,
            size: None,
            buffered_event: None,
            pending: PromptStatus::default(),
        }
    }

    pub fn contents(&self) -> &str {
        self.state.contents()
    }

    pub fn view(&self) -> (&str, u16) {
        if self
            .size
            .is_none_or(|size| size.width == 0 || size.height == 0)
        {
            return ("", 0);
        }
        self.state.view()
    }

    pub fn flush(&mut self) {
        if let Some(event) = self.buffered_event.take() {
            self.apply(event);
        }
    }

    fn apply(&mut self, event: PromptEvent) {
        let status = self.state.apply(event);
        self.pending.needs_redraw |= status.needs_redraw;
        self.pending.contents_changed |= status.contents_changed;
    }

    pub fn handle(&mut self, event: PromptEvent) {
        let Some(ref mut buffered) = self.buffered_event else {
            self.buffered_event = Some(event);
            return;
        };

        match (buffered, event) {
            (PromptEvent::Left(n1), PromptEvent::Left(n2))
            | (PromptEvent::WordLeft(n1), PromptEvent::WordLeft(n2))
            | (PromptEvent::Right(n1), PromptEvent::Right(n2))
            | (PromptEvent::WordRight(n1), PromptEvent::WordRight(n2))
            | (PromptEvent::Backspace(n1), PromptEvent::Backspace(n2))
            | (PromptEvent::Delete(n1), PromptEvent::Delete(n2))
            | (PromptEvent::BackspaceWord(n1), PromptEvent::BackspaceWord(n2)) => {
                *n1 = n1.saturating_add(n2);
            }
            (b, PromptEvent::ToStart) if b.is_cursor_movement() => {
                *b = PromptEvent::ToStart;
            }
            (b, PromptEvent::ToEnd) if b.is_cursor_movement() => {
                *b = PromptEvent::ToEnd;
            }
            (b, PromptEvent::ClearBefore)
                if matches!(
                    b,
                    PromptEvent::Backspace(_)
                        | PromptEvent::ClearBefore
                        | PromptEvent::BackspaceWord(_)
                ) =>
            {
                *b = PromptEvent::ClearBefore;
            }
            (b, PromptEvent::ClearAfter)
                if matches!(b, PromptEvent::Delete(_) | PromptEvent::ClearAfter) =>
            {
                *b = PromptEvent::ClearAfter;
            }
            (PromptEvent::Paste(current), PromptEvent::Insert(ch)) => {
                current.push(ch);
            }
            (PromptEvent::Paste(current), PromptEvent::Paste(ref s)) => {
                current.push_str(s);
            }
            (b, e) if matches!(e, PromptEvent::Reset(_)) => {
                *b = e;
            }
            (b, mut e) => {
                // move the incoming event into the buffer and handle the buffered event
                std::mem::swap(b, &mut e);
                self.apply(e);
            }
        }
    }

    pub fn update(&mut self) -> PromptStatus {
        self.flush();
        std::mem::take(&mut self.pending)
    }
}

impl Component for Prompt<'_> {
    fn resize(&mut self, size: Dimensions) {
        self.size = Some(size);
        self.state.resize(size.width.saturating_sub(2));
    }
    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error> {
        self.resize(Dimensions {
            width: surface.width(),
            height: surface.height(),
        });
        if surface.is_empty() {
            return Ok(());
        }
        surface.clear_line()?;
        surface.text(self.chars.prompt.encode_utf8(&mut [0; 4]), Style::new())?;

        if surface.width() >= 2 {
            surface.spaces(1, Style::new())?;
            let (contents, shift) = self.view();
            surface.spaces(shift, Style::new())?;
            surface.text(contents, Style::new())?;
        }
        Ok(())
    }
    fn cursor(&self) -> Option<Position> {
        let size = self.size?;
        (size.width != 0 && size.height != 0).then(|| Position {
            column: self
                .state
                .cursor_column()
                .saturating_add(2)
                .min(size.width - 1),
            row: 0,
        })
    }
}
