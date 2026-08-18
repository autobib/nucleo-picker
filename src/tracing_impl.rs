use crate::event::{Event, MatchListEvent, PromptEvent};

#[derive(Default)]
pub(crate) struct EventStats {
    received: u64,
    prompt_events: u64,
    match_list_events: u64,
    redraw_events: u64,
    terminal_event: bool,
}

impl EventStats {
    pub(crate) fn receive<A>(&mut self, event: &Event<A>) {
        self.received += 1;
        match event {
            Event::Prompt(_) => self.prompt_events += 1,
            Event::MatchList(_) => self.match_list_events += 1,
            Event::Redraw => self.redraw_events += 1,
            _ => {}
        }
    }

    pub(crate) fn record_terminal(&mut self) {
        self.terminal_event = true;
    }

    pub(crate) fn record(&self, span: &tracing::Span) {
        span.record("received", self.received);
        span.record("prompt_events", self.prompt_events);
        span.record("match_list_events", self.match_list_events);
        span.record("redraw_events", self.redraw_events);
        span.record("terminal_event", self.terminal_event);
    }
}

pub(crate) fn trace_picker_event<A>(event: &Event<A>) {
    if !tracing::enabled!(target: "nucleo_picker::event", tracing::Level::TRACE) {
        return;
    }
    let (event_kind, action, amount) = match event {
        Event::Prompt(event) => match event {
            PromptEvent::Left(n) => ("prompt", "left", Some(*n)),
            PromptEvent::WordLeft(n) => ("prompt", "word_left", Some(*n)),
            PromptEvent::Right(n) => ("prompt", "right", Some(*n)),
            PromptEvent::WordRight(n) => ("prompt", "word_right", Some(*n)),
            PromptEvent::Backspace(n) => ("prompt", "backspace", Some(*n)),
            PromptEvent::Delete(n) => ("prompt", "delete", Some(*n)),
            PromptEvent::BackspaceWord(n) => ("prompt", "backspace_word", Some(*n)),
            PromptEvent::Insert(_) => ("prompt", "insert", Some(1)),
            PromptEvent::Paste(s) => ("prompt", "paste", Some(s.chars().count())),
            PromptEvent::Reset(s) => ("prompt", "reset", Some(s.chars().count())),
            PromptEvent::ToStart => ("prompt", "to_start", None),
            PromptEvent::ToEnd => ("prompt", "to_end", None),
            PromptEvent::ClearBefore => ("prompt", "clear_before", None),
            PromptEvent::ClearAfter => ("prompt", "clear_after", None),
        },
        Event::MatchList(event) => match event {
            MatchListEvent::Up(n) => ("match_list", "up", Some(*n)),
            MatchListEvent::ToggleUp(n) => ("match_list", "toggle_up", Some(*n)),
            MatchListEvent::Down(n) => ("match_list", "down", Some(*n)),
            MatchListEvent::ToggleDown(n) => ("match_list", "toggle_down", Some(*n)),
            MatchListEvent::QueueAbove(n) => ("match_list", "queue_above", Some(*n)),
            MatchListEvent::QueueBelow(n) => ("match_list", "queue_below", Some(*n)),
            MatchListEvent::QueueMatches => ("match_list", "queue_matches", None),
            MatchListEvent::Unqueue => ("match_list", "unqueue", None),
            MatchListEvent::UnqueueAll => ("match_list", "unqueue_all", None),
            MatchListEvent::Reset => ("match_list", "reset", None),
        },
        Event::Quit => ("terminal", "quit", None),
        Event::QuitPromptEmpty => ("terminal", "quit_prompt_empty", None),
        Event::UserInterrupt => ("terminal", "user_interrupt", None),
        Event::Abort(_) => ("terminal", "abort", None),
        Event::Redraw => ("redraw", "redraw", None),
        Event::Select => ("terminal", "select", None),
        Event::Restart => ("picker", "restart", None),
    };
    tracing::event!(name: "picker.event", target: "nucleo_picker::event", tracing::Level::TRACE, event_kind, action, amount);
}
