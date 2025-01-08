use crate::{
    component::Component,
    event::{MatchListEvent, PromptEvent},
    match_list::MatchList,
    prompt::{Prompt, PromptStatus},
    util::as_u32,
    Render,
};

pub struct LazyMatchList<'a, T: Send + Sync + 'static, R: Render<T>> {
    match_list: &'a mut MatchList<T, R>,
    buffered_selection: u32,
}

impl<'a, T: Send + Sync + 'static, R: Render<T>> LazyMatchList<'a, T, R> {
    pub fn new(match_list: &'a mut MatchList<T, R>) -> Self {
        let buffered_selection = match_list.selection();
        Self {
            match_list,
            buffered_selection,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.match_list.is_empty()
    }

    pub fn selection(&self) -> u32 {
        self.buffered_selection
    }

    /// Handle an event.
    ///
    /// Note that this may not actually apply the event change to the underlying [`MatchList`]; you
    /// must call [`finish`](Self::finish) in order to guarantee that all events are fully
    /// processed.
    pub fn handle(&mut self, event: MatchListEvent) {
        match event {
            MatchListEvent::Up(n) => {
                self.buffered_selection = self
                    .buffered_selection
                    .saturating_add(as_u32(n))
                    .min(self.match_list.max_selection());
            }
            MatchListEvent::Down(n) => {
                self.buffered_selection = self.buffered_selection.saturating_sub(as_u32(n));
            }
            MatchListEvent::Reset => {
                self.buffered_selection = 0;
            }
        }
    }

    /// Complete processing and clear any buffered events.
    pub fn finish(self) -> bool {
        self.match_list.set_selection(self.buffered_selection)
    }
}

pub struct LazyPrompt<'a> {
    prompt: &'a mut Prompt,
    buffered_event: Option<PromptEvent>,
    status: PromptStatus,
}

impl<'a> LazyPrompt<'a> {
    pub fn is_empty(&self) -> bool {
        self.prompt.is_empty()
    }

    pub fn new(prompt: &'a mut Prompt) -> Self {
        Self {
            prompt,
            buffered_event: None,
            status: PromptStatus::default(),
        }
    }

    /// `self.buffered_event` must be Some()
    fn swap_and_process_buffer(&mut self, mut event: PromptEvent) {
        // put the 'new' event in the buffer, and move the 'buffered' event into new
        std::mem::swap(
            unsafe { self.buffered_event.as_mut().unwrap_unchecked() },
            &mut event,
        );
        // process the buffered event (now swapped)
        self.status |= self.prompt.handle(event);
    }

    pub fn finish(mut self) -> PromptStatus {
        if let Some(event) = self.buffered_event {
            self.status |= self.prompt.handle(event);
        }

        self.status
    }

    pub fn handle(&mut self, mut event: PromptEvent) {
        match self.buffered_event {
            None => {
                self.buffered_event = Some(event);
            }
            Some(ref mut buffered) => match event {
                PromptEvent::Left(ref mut n1) => {
                    if let PromptEvent::Left(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::WordLeft(ref mut n1) => {
                    if let PromptEvent::WordLeft(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Right(ref mut n1) => {
                    if let PromptEvent::Right(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::WordRight(ref mut n1) => {
                    if let PromptEvent::WordRight(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::ToStart => {
                    if buffered.is_cursor_movement() {
                        *buffered = PromptEvent::ToStart;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::ToEnd => {
                    if buffered.is_cursor_movement() {
                        *buffered = PromptEvent::ToEnd;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Backspace(ref mut n1) => {
                    if let PromptEvent::Backspace(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Delete(ref mut n1) => {
                    if let PromptEvent::Delete(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::BackspaceWord(ref mut n1) => {
                    if let PromptEvent::BackspaceWord(n2) = buffered {
                        *n1 += *n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::ClearBefore => {
                    if matches!(
                        buffered,
                        PromptEvent::Backspace(_)
                            | PromptEvent::ClearBefore
                            | PromptEvent::BackspaceWord(_)
                    ) {
                        *buffered = PromptEvent::ClearBefore;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::ClearAfter => {
                    if matches!(buffered, PromptEvent::Delete(_) | PromptEvent::ClearAfter) {
                        *buffered = PromptEvent::ClearAfter;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Insert(ch1) => match buffered {
                    PromptEvent::Insert(ch2) => {
                        let mut s = ch1.to_string();
                        s.push(*ch2);
                        *buffered = PromptEvent::Paste(s);
                    }
                    PromptEvent::Paste(new) => {
                        let mut s = ch1.to_string();
                        s.push_str(new);
                        *buffered = PromptEvent::Paste(s);
                    }
                    _ => {
                        self.swap_and_process_buffer(event);
                    }
                },
                PromptEvent::Paste(ref mut s) => match buffered {
                    PromptEvent::Insert(ch2) => {
                        s.push(*ch2);
                    }
                    PromptEvent::Paste(new) => {
                        s.push_str(new);
                    }
                    _ => {
                        self.swap_and_process_buffer(event);
                    }
                },
                PromptEvent::Set(_) => {
                    // a 'set' event overwrites any other event since it resets the buffer
                    *buffered = event;
                }
            },
        };
    }
}
