//! The internal generic pick implementation

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use crate::{
    Picker, Render, Terminal,
    component::{Component, PreviewComponent},
    error::PickError,
    event::{Event, EventSource, PickerStatus, RecvError},
    frame::{Frame, Redraw},
    match_engine::MatchEngine,
    match_list::{MatchList, Queued},
    prompt::Prompt,
    status_line::StatusLine,
    terminal::TerminalSession,
};

enum SelectionTarget {
    None,
    Current(u32),
    Queued,
}

impl<T: Send + Sync + 'static, R: Render<T>> Picker<T, R> {
    pub(crate) fn pick_impl<E, W, Q: Queued, P>(
        &mut self,
        mut event_source: E,
        writer: &mut W,
        mut preview: P,
    ) -> Result<Q::Output<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
        P: PreviewComponent<T, Error = E::AbortErr> + Component<MatchEngine<T, R>>,
    {
        let engine = &mut self.engine;
        let mut prompt = Prompt::new(&mut self.prompt, &self.chars);
        let mut list = MatchList::new(
            &mut self.list_state,
            &self.chars,
            Q::init(self.max_selection_count),
        );
        let mut status_line = StatusLine::new(&self.chars);
        let mut terminal = TerminalSession::new(writer);
        terminal.init()?;

        let mut frame_start = Instant::now();
        list.flush(engine);
        let initial = engine.update(5);
        list.update(engine, initial.items_changed, false);
        status_line.update(
            initial,
            list.queued().count(self.max_selection_count),
            false,
        );
        #[cfg(feature = "preview")]
        let preview_ratio = P::ENABLED.then_some(self.preview_config.ratio);
        #[cfg(not(feature = "preview"))]
        let preview_ratio = None;
        let mut frame = Frame::new(terminal.size()?, self.reversed, preview_ratio);
        frame.resize(
            engine,
            &mut prompt,
            &mut list,
            &mut status_line,
            &mut preview,
        );
        preview
            .update(
                if P::ENABLED {
                    list.selection(engine).map(|n| engine.idx_from_match(n))
                } else {
                    None
                },
                engine.snapshot(),
                frame_start + self.interval,
            )
            .map_err(PickError::Aborted)?;
        frame.draw(
            engine,
            &mut prompt,
            &mut list,
            &mut status_line,
            &mut preview,
            &mut terminal,
            Redraw::full(),
        )?;

        let mut frame_number = 0_u64;
        let mut handle_status = None;
        let selection = 'selection: loop {
            let mut force_redraw = false;
            let frame_deadline = frame_start + self.interval;
            // this is a deadline with a 1ms bonus so that we always have a bit of time to
            // process extra events even if there is almost no time left
            let drain_deadline = frame_deadline.max(Instant::now()) + Duration::from_millis(1);
            loop {
                // for ordinary receives, we timeout with the usual frame_deadline grace period
                // but if we get an event immediately, we make sure to process events for at least
                // 1ms extra in order to drain some events from a very overactive event queue
                let event = match event_source.try_recv() {
                    Err(RecvError::Timeout) => {
                        let now = Instant::now();
                        if now >= frame_deadline {
                            break;
                        }
                        event_source.recv_timeout(frame_deadline - now)
                    }
                    event => event,
                };

                let event = match event {
                    Ok(event) => event,
                    Err(RecvError::Timeout) => break,
                    Err(RecvError::Disconnected) => break 'selection Err(PickError::Disconnected),
                    Err(RecvError::IO(err)) => break 'selection Err(PickError::IO(err)),
                };

                match event {
                    Event::Prompt(event) => prompt.handle(event),
                    Event::MatchList(event) => list.handle(event, engine),
                    Event::Layout(event) => frame.handle(event),
                    #[cfg(feature = "preview")]
                    Event::Preview(event) => {
                        if P::ENABLED && frame.preview_enabled() {
                            preview.handle(
                                event,
                                list.event_selection(engine)
                                    .map(|n| engine.idx_from_match(n)),
                            );
                        }
                    }
                    Event::Redraw => force_redraw = true,
                    Event::Quit => break 'selection Ok(SelectionTarget::None),
                    Event::QuitPromptEmpty => {
                        prompt.flush();
                        if prompt.contents().is_empty() {
                            break 'selection Ok(SelectionTarget::None);
                        }
                    }
                    Event::Select => {
                        if !list.queued().is_empty() {
                            break 'selection Ok(SelectionTarget::Queued);
                        }
                        if let Some(n) = list.event_selection(engine) {
                            break 'selection Ok(SelectionTarget::Current(n));
                        }
                    }
                    Event::Status { id } => handle_status = Some(id),
                    Event::Restart => match self.restart_notifier {
                        Some(ref notifier) => {
                            preview.restart();
                            engine.restart();
                            list.restart(engine);
                            let injector = engine.injector();
                            if notifier.push(injector).is_err() {
                                break 'selection Err(PickError::Disconnected);
                            }
                        }
                        None => break 'selection Err(PickError::Disconnected),
                    },
                    Event::UserInterrupt => break 'selection Err(PickError::UserInterrupted),
                    Event::Abort(err) => break 'selection Err(PickError::Aborted(err)),
                }
                if Instant::now() >= drain_deadline {
                    break;
                }
            }
            frame_start = Instant::now();
            frame_number = frame_number.wrapping_add(1);
            let background_frame =
                frame_number.is_multiple_of(self.background_frame_frequency.get() as u64);
            // prompt first, since this drives all of the other changes
            let prompt_change = prompt.update();

            // make sure that the buffered changes target the match engine
            // *before* it updates
            list.flush(engine);
            if prompt_change.contents_changed {
                engine.reparse(prompt.contents());
            }

            // then update the match engine
            let status = engine.update(2 * self.interval.as_millis() as u64 / 3);

            // update other components with the new prompt/engine state and process redraws
            let mut redraw = Redraw {
                prompt: prompt_change.needs_redraw,
                list: list.update(engine, status.items_changed, prompt_change.contents_changed),
                status: status_line.update(
                    status,
                    list.queued().count(self.max_selection_count),
                    background_frame,
                ) && frame.status_enabled(),
                preview: P::ENABLED
                    && frame.preview_enabled()
                    && preview
                        .update(
                            list.selection(engine).map(|n| engine.idx_from_match(n)),
                            engine.snapshot(),
                            frame_start + self.interval,
                        )
                        .map_err(PickError::Aborted)?,
            };

            // a redraw was externally requested
            if force_redraw {
                redraw = Redraw::full();
            }

            // render the frame
            let changed = frame.render(
                engine,
                &mut prompt,
                &mut list,
                &mut status_line,
                &mut preview,
                &mut terminal,
                redraw,
            )?;
            terminal.end_frame(changed)?;

            // handle the status event but do not fail if the status notifier
            // is disconnected
            if let Some(id) = handle_status.take()
                && let Some(ref notifier) = self.status_notifier
            {
                let (width, height) = frame.dimensions();
                let _ = notifier.push(PickerStatus {
                    id,
                    query: prompt.contents().to_owned(),
                    changed,
                    selection: list.selection(engine),
                    item_count: status.total,
                    selected_item_count: list.queued().len(),
                    matched_item_count: status.matched,
                    width,
                    height,
                    matching: status.matching,
                    injecting: status.injecting,
                });
            }
        };

        if prompt.update().contents_changed {
            engine.reparse(prompt.contents());
        }
        terminal.finish()?;

        // process and return the selection
        let selection = selection?;
        let mut queued = list.into_queue();
        match selection {
            SelectionTarget::None => {
                queued.clear();
                Ok(queued.into_selection(engine.snapshot()))
            }
            SelectionTarget::Current(n) => {
                Ok(queued.into_only_selection(engine.snapshot(), engine.idx_from_match(n)))
            }
            SelectionTarget::Queued => Ok(queued.into_selection(engine.snapshot())),
        }
    }
}
