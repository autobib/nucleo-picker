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

pub struct LazyMatchListStatus {
    pub selection_changed: bool,
    pub queued_changed: bool,
}

impl<'a, T: Send + Sync + 'static, R: Render<T>, Q: crate::Queued> LazyMatchList<'a, T, R, Q> {
    pub fn new(match_list: &'a mut MatchList<T, R>, queued: &'a mut Q) -> Self {
        let buffered_selection = match_list.raw_selection();
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
                    }
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
                    }
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
    pub fn finish(self) -> LazyMatchListStatus {
        LazyMatchListStatus {
            selection_changed: self.match_list.set_selection(self.buffered_selection),
            queued_changed: self.toggled,
        }
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

    pub fn flush(&mut self) {
        if let Some(event) = self.buffered_event.take() {
            self.status |= self.prompt.handle(event);
        }
    }

    pub fn finish(mut self) -> PromptStatus {
        self.flush();
        self.status
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
                self.status |= self.prompt.handle(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Picker,
        event::MatchListEvent,
        match_list::{Queued, SelectedIndices},
        render::StrRenderer,
    };

    use super::LazyMatchList;

    fn picker() -> Picker<&'static str, StrRenderer> {
        let mut picker = Picker::new(StrRenderer);
        picker.extend(["a", "b"]);
        while picker.match_list.update(5).matching {}
        picker
    }

    #[test]
    fn selection_changes_do_not_change_the_queue() {
        let mut picker = picker();
        let mut queued = SelectedIndices::init(None);
        let mut lazy = LazyMatchList::new(&mut picker.match_list, &mut queued);
        lazy.handle(MatchListEvent::Up(1));

        let status = lazy.finish();

        assert!(status.selection_changed);
        assert!(!status.queued_changed);
    }

    #[test]
    fn queue_changes_are_reported_separately() {
        let mut picker = picker();
        let mut queued = SelectedIndices::init(None);
        let mut lazy = LazyMatchList::new(&mut picker.match_list, &mut queued);
        lazy.handle(MatchListEvent::ToggleDown(0));

        let status = lazy.finish();

        assert!(!status.selection_changed);
        assert!(status.queued_changed);
    }
}
