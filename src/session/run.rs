//! The internal generic pick implementation

use std::{
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Context, Exit};
use crate::{
    PickerCore, Render, Terminal,
    component::PreviewComponent,
    error::PickError,
    event::{Event, EventSource, PickerStatus, RecvError},
    frame::{Frame, Redraw},
    match_engine::MatchEngine,
    match_list::{MatchList, QueueEvent, Queued},
    prompt::Prompt,
    status_line::StatusLine,
    terminal::session,
    util::as_u32,
};

impl<T: Send + Sync + 'static, R: Render<T>> PickerCore<T, R> {
    pub(crate) fn pick_impl<E, W, Q: Queued, P>(
        &mut self,
        mut event_source: E,
        writer: &mut W,
        mut queued: Q,
        preview: &mut P,
    ) -> Result<Q::Output<'_, T>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
        P: PreviewComponent<T>,
    {
        let mut preview_session = DropGuard::new(
            (preview.init(), preview),
            |mut session: (P::State, &mut P)| {
                if !std::thread::panicking() {
                    session.1.end(&mut session.0);
                }
            },
        );
        let (preview_state, preview) = &mut *preview_session;
        let engine = &mut self.engine;
        // Headless updates defer layout work until the next interactive session.
        if std::mem::take(&mut self.list_state.items_changed) {
            self.list_state
                .layout
                .update_items(engine.snapshot(), &self.list_state.config);
        }
        let mut prompt = Prompt::new(&mut self.prompt, &self.chars);
        let mut list = MatchList::new(&mut self.list_state, &self.chars);
        let mut status_line = StatusLine::new(&self.chars);
        let mut terminal = session(writer);
        terminal.init()?;

        let mut frame_start = Instant::now();
        list.flush(engine);
        let initial = engine.update(5);
        list.update(engine, initial.items_changed, false);
        status_line.update(
            initial,
            queued.count().map(|count| (as_u32(count), queued.limit())),
            false,
        );
        #[cfg(feature = "preview")]
        let preview_ratio = P::ENABLED.then_some(self.preview_config.ratio);
        #[cfg(not(feature = "preview"))]
        let preview_ratio = None;
        let mut frame = Frame::new(terminal.dimensions()?, self.reversed, preview_ratio);
        frame.resize(
            &(&*engine, &queued),
            &mut prompt,
            &mut list,
            &mut status_line,
            &mut preview.component(
                preview_state,
                &self.chars,
                #[cfg(feature = "preview")]
                &self.preview_config,
            ),
        );
        preview.update(
            preview_state,
            &context(engine, list.rank(engine), &frame),
            true,
            frame_start + self.interval,
        );
        frame.draw(
            &(&*engine, &queued),
            &mut prompt,
            &mut list,
            &mut status_line,
            &mut preview.component(
                preview_state,
                &self.chars,
                #[cfg(feature = "preview")]
                &self.preview_config,
            ),
            &mut **terminal,
            Redraw::full(),
        )?;
        if P::ENABLED && frame.preview_enabled() {
            preview.prefetch(
                preview_state,
                &context(engine, list.rank(engine), &frame),
                frame_start + self.interval,
            );
        }

        let mut frame_number = 0_u64;
        let mut handle_status = None;
        let mut changed_since_status = true;
        let mut matches_changed = false;
        let exit = 'selection: loop {
            // whether the preview was enabled on the previous frame
            let preview_was_enabled = frame.preview_enabled();
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
                        // on a force redraw don't wait for more events: immediately handle the
                        // redraw. this can reduce rendering flashes (where the terminal repaints
                        // before the layout is updated)
                        if force_redraw || now >= frame_deadline {
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
                    Event::MatchList(event) => {
                        list.handle(
                            event,
                            &mut queued,
                            &context(engine, list.event_rank(engine), &frame),
                        );
                    }
                    Event::Layout(event) => frame.handle(event),
                    #[cfg(feature = "preview")]
                    Event::Preview(event) => {
                        if P::ENABLED && frame.preview_enabled() {
                            // we have a previewer (i.e.: inside a PreviewPicker) and moreover
                            // the frame has not been hidden by the user.

                            // note that this not check *visibility* of the preview frame
                            // since that would bleed state from the frame logic everywhere
                            // which is a lot of complexity which is only required for
                            // unrealistically tiny terminals
                            preview.handle(
                                preview_state,
                                event,
                                &context(engine, list.event_rank(engine), &frame),
                            );
                        }
                    }
                    Event::Redraw => force_redraw = true,
                    Event::Quit => break 'selection Ok(Exit::Quit),
                    Event::QuitPromptEmpty => {
                        prompt.flush();
                        if prompt.contents().is_empty() {
                            break 'selection Ok(Exit::Quit);
                        }
                    }
                    Event::Select => {
                        let hovered = list
                            .event_rank(engine)
                            .map(|rank| engine.item_id(engine.idx_from_match(rank)));
                        if queued.count().is_some_and(|count| count != 0) || hovered.is_some() {
                            break 'selection Ok(Exit::Selected(hovered));
                        }
                    }
                    Event::Status { id } => handle_status = Some(id),
                    Event::Restart => match self.restart_notifier {
                        Some(ref notifier) => {
                            preview.restart(preview_state);
                            engine.restart();
                            matches_changed = true;
                            let cx = context(engine, None, &frame);
                            list.restart(&cx);
                            queued.on_queue(QueueEvent::Clear, &cx);
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
            matches_changed |= status.items_changed;

            // update other components with the new prompt/engine state and process redraws
            let mut redraw = Redraw {
                prompt: prompt_change.needs_redraw,
                list: list.update(engine, status.items_changed, prompt_change.contents_changed),
                status: status_line.update(
                    status,
                    queued.count().map(|count| (as_u32(count), queued.limit())),
                    background_frame,
                ) && frame.status_enabled(),
                preview: preview.update(
                    preview_state,
                    &context(engine, list.rank(engine), &frame),
                    std::mem::take(&mut matches_changed),
                    frame_start + self.interval,
                ) && P::ENABLED
                    && frame.preview_enabled(),
            };

            if P::ENABLED && preview_was_enabled && !frame.preview_enabled() {
                // this is only used to process `hide` events within the previewer;
                // currently this controls the 'hover_changed' event
                preview.hide(preview_state);
            }

            // a redraw was externally requested
            if force_redraw {
                redraw = Redraw::full();
            }

            let changed = frame.render(
                &(&*engine, &queued),
                &mut prompt,
                &mut list,
                &mut status_line,
                &mut preview.component(
                    preview_state,
                    &self.chars,
                    #[cfg(feature = "preview")]
                    &self.preview_config,
                ),
                &mut **terminal,
                redraw,
            )?;

            changed_since_status |= changed;
            terminal.end_frame(changed)?;

            // handle the status event but do not fail if the status notifier
            // is disconnected
            if let Some(id) = handle_status.take()
                && let Some(ref notifier) = self.status_notifier
            {
                let _ = notifier.push(PickerStatus {
                    id,
                    query: prompt.contents().to_owned(),
                    changed: std::mem::take(&mut changed_since_status),
                    frame_changed: changed,
                    rank: list.rank(engine),
                    item_count: status.item_count,
                    selected_item_count: queued.count().map_or(0, as_u32),
                    matched_item_count: status.matched_item_count,
                    dimensions: frame.dimensions(),
                    matching: status.matching,
                    injecting: status.injecting,
                });
            }
            if P::ENABLED && frame.preview_enabled() {
                preview.prefetch(
                    preview_state,
                    &context(engine, list.rank(engine), &frame),
                    frame_start + self.interval,
                );
            }
        };

        list.flush(engine);
        if prompt.update().contents_changed {
            engine.reparse(prompt.contents());
        }
        let cleanup = terminal.cleanup();
        DropGuard::dismiss(terminal);
        cleanup?;

        // process and return the selection
        let exit = exit?;
        drop(preview_session);
        let cx = context(engine, list.rank(engine), &frame);
        Ok(queued.on_select(exit, cx))
    }
}

fn context<'items, T: Send + Sync + 'static, R>(
    engine: &'items MatchEngine<T, R>,
    rank: Option<u32>,
    frame: &Frame,
) -> Context<'items, T> {
    let cx = Context::new(engine).with_rank(rank);
    #[cfg(feature = "preview")]
    let cx = cx.with_preview(
        frame
            .preview_enabled()
            .then_some(frame.preview_dimensions()),
    );
    #[cfg(not(feature = "preview"))]
    let _ = frame;
    cx
}
