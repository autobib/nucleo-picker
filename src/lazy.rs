use crate::{
    Injector, Render,
    event::{MatchListEvent, PromptEvent},
    match_list::MatchList,
    prompt::{Prompt, PromptStatus},
    util::as_u32,
};

pub struct LazyMatchList<'a, T: Send + Sync + 'static, R: Render<T>, Q> {
    match_list: &'a mut MatchList<T, R>,
    queued: &'a mut Q,
    buffered_selection: u32,
    toggled: bool,
}

impl<'a, T: Send + Sync + 'static, R: Render<T>, Q: crate::Queued> LazyMatchList<'a, T, R, Q> {
    pub fn new(match_list: &'a mut MatchList<T, R>, queued: &'a mut Q) -> Self {
        let buffered_selection = match_list.selection();
        Self {
            match_list,
            buffered_selection,
            queued,
            toggled: false,
        }
    }

    pub fn restart(&mut self) -> Injector<T, R> {
        self.match_list.restart();
        self.queued.clear();
        self.buffered_selection = 0;
        self.match_list.injector()
    }

    pub fn is_empty(&self) -> bool {
        self.match_list.is_empty()
    }

    pub fn has_queued_items(&self) -> bool {
        !self.queued.is_empty()
    }

    pub fn selection(&self) -> Option<u32> {
        if self.is_empty() {
            None
        } else {
            Some(self.buffered_selection)
        }
    }

    pub fn toggle_selection(&mut self) -> bool {
        if !self.is_empty()
            && self
                .match_list
                .toggle_queued_item(self.queued, self.buffered_selection)
        {
            self.toggled = true;
            true
        } else {
            false
        }
    }

    fn decr(&mut self, n: usize) {
        self.buffered_selection = self.buffered_selection.saturating_sub(as_u32(n));
    }

    fn incr(&mut self, n: usize) {
        self.buffered_selection = self
            .buffered_selection
            .saturating_add(as_u32(n))
            .min(self.match_list.max_selection());
    }

    /// Handle an event.
    ///
    /// Note that this may not actually apply the event change to the underlying [`MatchList`]; you
    /// must call [`finish`](Self::finish) in order to guarantee that all events are fully
    /// processed.
    #[inline]
    pub fn handle(&mut self, event: MatchListEvent) {
        match event {
            MatchListEvent::Up(n) => {
                if self.match_list.reversed() {
                    self.decr(n);
                } else {
                    self.incr(n);
                }
            }
            MatchListEvent::ToggleUp(n) => {
                if self.toggle_selection() {
                    if self.match_list.reversed() {
                        self.decr(n);
                    } else {
                        self.incr(n);
                    }
                }
            }
            MatchListEvent::Down(n) => {
                if self.match_list.reversed() {
                    self.incr(n);
                } else {
                    self.decr(n);
                }
            }
            MatchListEvent::QueueAbove(n) => {
                if !self.is_empty() {
                    if self.match_list.reversed() {
                        let (shift, toggled) = self.match_list.queue_items_below(
                            self.queued,
                            self.buffered_selection,
                            n,
                        );
                        self.toggled |= toggled;
                        self.decr(shift.saturating_sub(1));
                    } else {
                        let (shift, toggled) = self.match_list.queue_items_above(
                            self.queued,
                            self.buffered_selection,
                            n,
                        );
                        self.toggled |= toggled;
                        self.incr(shift.saturating_sub(1));
                    };
                }
            }
            MatchListEvent::QueueBelow(n) => {
                if !self.is_empty() {
                    if self.match_list.reversed() {
                        let (shift, toggled) = self.match_list.queue_items_above(
                            self.queued,
                            self.buffered_selection,
                            n,
                        );
                        self.toggled |= toggled;
                        self.incr(shift.saturating_sub(1));
                    } else {
                        let (shift, toggled) = self.match_list.queue_items_below(
                            self.queued,
                            self.buffered_selection,
                            n,
                        );
                        self.toggled |= toggled;
                        self.decr(shift.saturating_sub(1));
                    };
                }
            }
            MatchListEvent::QueueMatches => {
                self.toggled |= self.match_list.queue_all(self.queued);
            }
            MatchListEvent::Unqueue => {
                self.toggled |= !self.is_empty()
                    && self
                        .match_list
                        .unqueue_item(self.queued, self.buffered_selection);
            }
            MatchListEvent::UnqueueAll => {
                self.toggled |= self.queued.clear();
            }
            MatchListEvent::ToggleDown(n) => {
                if self.toggle_selection() {
                    if self.match_list.reversed() {
                        self.incr(n);
                    } else {
                        self.decr(n);
                    }
                }
            }
            MatchListEvent::Reset => {
                self.buffered_selection = 0;
            }
        }
    }

    /// Complete processing and clear any buffered events.
    pub fn finish(self) -> bool {
        self.match_list.set_selection(self.buffered_selection) || self.toggled
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

    pub fn handle(&mut self, event: PromptEvent) {
        match self.buffered_event {
            None => {
                self.buffered_event = Some(event);
            }
            Some(ref mut buffered) => match event {
                PromptEvent::Left(n2) => {
                    if let PromptEvent::Left(n1) = buffered {
                        *n1 += n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::WordLeft(n2) => {
                    if let PromptEvent::WordLeft(n1) = buffered {
                        *n1 += n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Right(n2) => {
                    if let PromptEvent::Right(n1) = buffered {
                        *n1 += n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::WordRight(n2) => {
                    if let PromptEvent::WordRight(n1) = buffered {
                        *n1 += n2;
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
                PromptEvent::Backspace(n2) => {
                    if let PromptEvent::Backspace(n1) = buffered {
                        *n1 += n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::Delete(n2) => {
                    if let PromptEvent::Delete(n1) = buffered {
                        *n1 += n2;
                    } else {
                        self.swap_and_process_buffer(event);
                    }
                }
                PromptEvent::BackspaceWord(n2) => {
                    if let PromptEvent::BackspaceWord(n1) = buffered {
                        *n1 += n2;
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
                PromptEvent::Insert(ch) => match buffered {
                    PromptEvent::Paste(new) => {
                        new.push(ch);
                    }
                    _ => {
                        self.swap_and_process_buffer(event);
                    }
                },
                PromptEvent::Paste(ref s) => match buffered {
                    PromptEvent::Paste(new) => {
                        new.push_str(s);
                    }
                    _ => {
                        self.swap_and_process_buffer(event);
                    }
                },
                PromptEvent::Reset(_) => {
                    // a 'set' event overwrites any other event since it resets the buffer
                    *buffered = event;
                }
            },
        };
    }
}
