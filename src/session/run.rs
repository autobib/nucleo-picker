//! The internal generic pick implementation

use std::{
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Context, Exit, Interface, PickerCore, Redraw};
use crate::{
    Render, Terminal,
    component::Component,
    error::PickError,
    event::{Event, EventSource, PickerStatus, RecvError},
    frame::{Contents, Frame},
    match_engine::MatchEngine,
    match_list::{MatchList, MatchListView},
    prompt::Prompt,
    rect::Area,
    status_line::StatusLine,
    terminal::session,
};

impl<T: Send + Sync + 'static, R: Render<T>> PickerCore<T, R> {
    pub(crate) fn pick_impl<E, W, I: Interface<T>>(
        &mut self,
        mut event_source: E,
        writer: &mut W,
        interface: &mut I,
    ) -> Result<I::Output<'_>, PickError<E::AbortErr>>
    where
        E: EventSource,
        W: Terminal,
    {
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
        let match_budget = 2 * self.interval.as_millis() as u64 / 3;
        let mut frame_start = Instant::now();
        list.flush(engine);
        let initial = engine.update(match_budget);
        list.update(engine, initial.items_changed, false);
        let has_preview = I::PREVIEW;
        let cx = Context::new(engine)
            .with_rank(list.rank(engine))
            .with_preview_enabled(has_preview);
        let mut session_state = DropGuard::new(
            (interface.init(&cx), interface),
            |mut session: (I::State, &mut I)| {
                if !std::thread::panicking() {
                    session.1.on_end(&mut session.0);
                }
            },
        );
        let (state, interface) = &mut *session_state;
        let mut terminal = session(writer);
        terminal.init()?;

        // The initial full repaint includes all changes reported by before_frame.
        let _ = interface.before_frame(state, &cx, true, frame_start + self.interval);
        status_line.update(initial, interface.queue_status(state), false);
        let preview_ratio = has_preview.then_some(self.preview_config.ratio);
        let mut frame = Frame::new(terminal.dimensions()?, self.reversed, preview_ratio);
        let mut contents = SessionContents {
            engine,
            interface: &mut **interface,
            state,
            prompt: &mut prompt,
            list: &mut list,
            status: &mut status_line,
            chars: &self.chars,
            preview_config: &self.preview_config,
        };
        frame.resize(&mut contents);
        frame.draw(&mut contents, &mut **terminal, Redraw::FULL)?;
        let mut pending_redraw = interface.after_frame(state, &cx, frame_start + self.interval);

        let mut frame_number = 0_u64;
        let mut handle_status = None;
        let mut changed_since_status = true;
        let mut matches_changed = false;
        let exit = 'selection: loop {
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
                            &mut **interface,
                            state,
                            &context(engine, list.event_rank(engine), &frame),
                        );
                    }
                    Event::Layout(event) => frame.handle(event),
                    Event::Preview(event) => {
                        if frame.preview_enabled() {
                            // we have a previewer (i.e.: inside a PreviewPicker) and moreover
                            // the frame has not been hidden by the user.

                            // note that this not check *visibility* of the preview frame
                            // since that would bleed state from the frame logic everywhere
                            // which is a lot of complexity which is only required for
                            // unrealistically tiny terminals
                            pending_redraw |= interface.preview_event(
                                state,
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
                        if interface
                            .queue_status(state)
                            .is_some_and(|queue| queue.count != 0)
                            || hovered.is_some()
                        {
                            break 'selection Ok(Exit::Selected(hovered));
                        }
                    }
                    Event::Status { id } => handle_status = Some(id),
                    Event::Restart => match self.restart_notifier {
                        Some(ref notifier) => {
                            interface.on_restart(
                                state,
                                &context(engine, list.event_rank(engine), &frame),
                            );
                            pending_redraw |= Redraw::QUEUE | Redraw::PREVIEW;
                            engine.restart();
                            matches_changed = true;
                            let cx = context(engine, None, &frame);
                            list.restart(&cx);
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
            let status = engine.update(match_budget);
            matches_changed |= status.items_changed;

            // update other components with the new prompt/engine state and process redraws
            pending_redraw.prompt |= prompt_change.needs_redraw;
            pending_redraw.list |=
                list.update(engine, status.items_changed, prompt_change.contents_changed);
            let cx = context(engine, list.rank(engine), &frame);
            pending_redraw |= interface.before_frame(
                state,
                &cx,
                std::mem::take(&mut matches_changed),
                frame_start + self.interval,
            );
            pending_redraw.status |=
                status_line.update(status, interface.queue_status(state), background_frame);
            let mut redraw = std::mem::take(&mut pending_redraw);
            redraw.status &= frame.status_enabled();
            redraw.preview &= frame.preview_enabled();

            // a redraw was externally requested
            if force_redraw {
                redraw = Redraw::FULL;
            }

            let changed = frame.render(
                &mut SessionContents {
                    engine,
                    interface: &mut **interface,
                    state,
                    prompt: &mut prompt,
                    list: &mut list,
                    status: &mut status_line,
                    chars: &self.chars,
                    preview_config: &self.preview_config,
                },
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
                    selected_item_count: interface
                        .queue_status(state)
                        .map_or(0, |queue| queue.count),
                    matched_item_count: status.matched_item_count,
                    dimensions: frame.dimensions(),
                    matching: status.matching,
                    injecting: status.injecting,
                });
            }
            pending_redraw |= interface.after_frame(state, &cx, frame_start + self.interval);
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
        let (mut state, interface) = DropGuard::dismiss(session_state);
        if !std::thread::panicking() {
            interface.on_end(&mut state);
        }
        let cx = context(engine, list.rank(engine), &frame);
        Ok(interface.finish(state, exit, cx))
    }
}

struct SessionContents<'a, 'session, T, R, I: Interface<T>> {
    engine: &'a MatchEngine<T, R>,
    interface: &'a mut I,
    state: &'a mut I::State,
    prompt: &'a mut Prompt<'session>,
    list: &'a mut MatchList<'session>,
    status: &'a mut StatusLine<'session>,
    chars: &'a crate::PickerChars,
    preview_config: &'a crate::preview::PreviewConfig,
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Contents
    for SessionContents<'_, '_, T, R, I>
{
    fn prompt(&mut self) -> impl Component {
        &mut *self.prompt
    }

    fn list(&mut self) -> impl Component {
        MatchListView {
            list: &mut *self.list,
            engine: self.engine,
            interface: &*self.interface,
            state: &*self.state,
        }
    }

    fn status(&mut self) -> impl Component {
        &mut *self.status
    }

    fn preview(&mut self) -> impl Component {
        crate::component::PreviewPane {
            interface: &*self.interface,
            state: &*self.state,
            boundary_chars: self.preview_config.boundary_chars,
            ellipsis: self.chars.ellipsis,
        }
    }

    fn preview_resize(&mut self, area: Option<Area>) {
        if I::PREVIEW {
            self.interface
                .preview_resize(self.state, area.map(crate::preview::interior_dimensions));
        }
    }
}

fn context<'items, T: Send + Sync + 'static, R>(
    engine: &'items MatchEngine<T, R>,
    rank: Option<u32>,
    frame: &Frame,
) -> Context<'items, T> {
    Context::new(engine)
        .with_rank(rank)
        .with_preview_enabled(frame.preview_enabled())
}
