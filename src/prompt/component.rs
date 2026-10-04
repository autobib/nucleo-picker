use super::{PromptEvent, PromptState, state::PromptStatus};
use crate::{
    PickerChars,
    component::Component,
    rect::{Area, Position, Rect},
};
use std::io;

pub(crate) struct Prompt<'a> {
    state: &'a mut PromptState,
    chars: &'a PickerChars,
    area: Area,
    buffered_event: Option<PromptEvent>,
    pending: PromptStatus,
}

impl<'a> Prompt<'a> {
    pub fn new(state: &'a mut PromptState, chars: &'a PickerChars) -> Self {
        Self {
            state,
            chars,
            area: Area::default(),
            buffered_event: None,
            pending: PromptStatus::default(),
        }
    }

    pub fn contents(&self) -> &str {
        self.state.contents()
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
            | (PromptEvent::BackspaceWord(n1), PromptEvent::BackspaceWord(n2)) => *n1 += n2,
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

impl<E> Component<E> for Prompt<'_> {
    fn resize(&mut self, area: Area, _engine: &E) {
        self.area = area;
        if let Some(width) = area.width.checked_sub(2) {
            self.state.resize(width);
        }
    }
    fn draw<D: Rect>(&mut self, _engine: &E, rect: &mut D) -> io::Result<()> {
        rect.clear_line()?;
        rect.print(self.chars.prompt)?;

        if rect.width().get() >= 2 {
            rect.print(" ")?;
            let (contents, shift) = self.state.view();
            rect.spaces(shift)?;
            rect.print(contents)?;
        }
        Ok(())
    }
    fn cursor(&self, _engine: &E) -> Option<Position> {
        (!self.area.is_empty()).then(|| Position {
            column: self
                .state
                .cursor_column()
                .saturating_add(2)
                .min(self.area.width - 1),
            row: 0,
        })
    }
}
