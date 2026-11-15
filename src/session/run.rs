//! The internal generic pick implementation

use std::{
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Context, Exit, Interface};
use crate::{
    PickerChars, PickerCore, Render, Terminal,
    component::Component,
    error::PickError,
    event::{Event, EventSource, PickerStatus, RecvError},
    frame::{Contents, Frame, Redraw},
    match_engine::MatchEngine,
    match_list::MatchList,
    prompt::Prompt,
    status_line::StatusLine,
    terminal::session,
    util::as_u32,
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
        #[cfg(feature = "preview")]
        let has_preview = I::HAS_PREVIEW;
        #[cfg(not(feature = "preview"))]
        let has_preview = false;
        let cx = Context::new(&self.engine);
        #[cfg(feature = "preview")]
        let cx = cx.with_preview(has_preview.then_some(super::Dimensions::default()));
        let mut session_state = DropGuard::new(
            (interface.init(&cx), interface),
            |mut session: (I::State, &mut I)| {
                if !std::thread::panicking() {
                    session.1.on_end(&mut session.0);
                }
            },
        );
        let (state, interface) = &mut *session_state;
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
            interface
                .queued_count(state)
                .map(|count| (as_u32(count), interface.queue_limit(state))),
            false,
        );
        #[cfg(feature = "preview")]
        let preview_ratio = has_preview.then_some(self.preview_config.ratio);
        #[cfg(not(feature = "preview"))]
        let preview_ratio = None;
        let mut frame = Frame::new(terminal.dimensions()?, self.reversed, preview_ratio);
        frame.resize(&mut SessionContents {
            engine,
            interface: &mut **interface,
            state,
            prompt: &mut prompt,
            list: &mut list,
            status: &mut status_line,
            chars: &self.chars,
            #[cfg(feature = "preview")]
            preview_config: &self.preview_config,
        });
        interface.update(
            state,
            &context(engine, list.rank(engine), &frame),
            true,
            frame_start + self.interval,
        );
        frame.draw(
            &mut SessionContents {
                engine,
                interface: &mut **interface,
                state,
                prompt: &mut prompt,
                list: &mut list,
                status: &mut status_line,
                chars: &self.chars,
                #[cfg(feature = "preview")]
                preview_config: &self.preview_config,
            },
            &mut **terminal,
            Redraw::full(),
        )?;
        if has_preview && frame.preview_enabled() {
            interface.prefetch(
                state,
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
                            &mut **interface,
                            state,
                            &context(engine, list.event_rank(engine), &frame),
                        );
                    }
                    Event::Layout(event) => frame.handle(event),
                    #[cfg(feature = "preview")]
                    Event::Preview(event) => {
                        if has_preview && frame.preview_enabled() {
                            // we have a previewer (i.e.: inside a PreviewPicker) and moreover
                            // the frame has not been hidden by the user.

                            // note that this not check *visibility* of the preview frame
                            // since that would bleed state from the frame logic everywhere
                            // which is a lot of complexity which is only required for
                            // unrealistically tiny terminals
                            interface.handle(
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
                            .queued_count(state)
                            .is_some_and(|count| count != 0)
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
            let status = engine.update(2 * self.interval.as_millis() as u64 / 3);
            matches_changed |= status.items_changed;

            // update other components with the new prompt/engine state and process redraws
            let mut redraw = Redraw {
                prompt: prompt_change.needs_redraw,
                list: list.update(engine, status.items_changed, prompt_change.contents_changed),
                status: status_line.update(
                    status,
                    interface
                        .queued_count(state)
                        .map(|count| (as_u32(count), interface.queue_limit(state))),
                    background_frame,
                ) && frame.status_enabled(),
                preview: interface.update(
                    state,
                    &context(engine, list.rank(engine), &frame),
                    std::mem::take(&mut matches_changed),
                    frame_start + self.interval,
                ) && has_preview
                    && frame.preview_enabled(),
            };

            if has_preview && preview_was_enabled && !frame.preview_enabled() {
                // this is only used to process `hide` events within the previewer;
                // currently this controls the 'hover_changed' event
                interface.hide(state);
            }

            // a redraw was externally requested
            if force_redraw {
                redraw = Redraw::full();
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
                    #[cfg(feature = "preview")]
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
                    selected_item_count: interface.queued_count(state).map_or(0, as_u32),
                    matched_item_count: status.matched_item_count,
                    dimensions: frame.dimensions(),
                    matching: status.matching,
                    injecting: status.injecting,
                });
            }
            if has_preview && frame.preview_enabled() {
                interface.prefetch(
                    state,
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
    chars: &'a PickerChars,
    #[cfg(feature = "preview")]
    preview_config: &'a crate::preview::PreviewConfig,
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Contents
    for SessionContents<'_, '_, T, R, I>
{
    fn prompt(&mut self) -> impl Component<()> {
        &mut *self.prompt
    }

    fn list(&mut self) -> impl Component<()> {
        (
            &mut *self.list,
            (self.engine, &*self.interface, &*self.state),
        )
    }

    fn status(&mut self) -> impl Component<()> {
        &mut *self.status
    }

    fn preview(&mut self) -> impl Component<()> {
        self.interface.component(
            self.state,
            self.chars,
            #[cfg(feature = "preview")]
            self.preview_config,
        )
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
