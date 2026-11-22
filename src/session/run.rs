//! The internal generic pick implementation

use std::{
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Driver, Exit, FrameOptions, Session};
use crate::{
    Render, Terminal,
    error::PickError,
    event::{Event, EventSource, LayoutEvent, PickerStatus, RecvError},
    frame::Layout,
    interface::Interface,
    rect::Dimensions,
    terminal::session,
};

impl<T: Send + Sync + 'static, R: Render<T>> Driver<T, R> {
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
        let match_budget = self.match_budget();
        let mut frame_start = Instant::now();
        let mut picker = Session::new(
            &mut self.engine,
            &mut self.list_state,
            &mut self.prompt,
            &self.chars,
            &self.preview_config,
            interface,
            match_budget,
        );
        let mut terminal = session(writer);
        terminal.init()?;

        // The initial full repaint includes all changes reported by before_frame.
        let mut layout = Layout::new(
            Dimensions::default(),
            self.reversed,
            I::PREVIEW.then_some(self.preview_config.ratio),
        );
        let mut frame = picker.prepare_frame(FrameOptions {
            deadline: frame_start + self.interval,
            background: false,
            visible_components: layout.visible_components(I::PREVIEW),
            layout_changed: false,
            force_redraw: false,
        });
        frame.resize(layout.geometry(terminal.dimensions()?, I::PREVIEW));
        frame.draw(&mut **terminal, frame.redraw())?;
        frame.complete();

        let mut frame_number = 0_u64;
        let mut handle_status = None;
        let mut changed_since_status = true;
        let exit = 'selection: loop {
            let mut force_redraw = false;
            let frame_deadline = frame_start + self.interval;
            // this is a deadline with a 1ms bonus so that we always have a bit of time to
            // process extra events even if there is almost no time left
            let drain_deadline = frame_deadline.max(Instant::now()) + Duration::from_millis(1);
            let mut batch = picker.events();
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
                    Event::Prompt(event) => batch.prompt(event),
                    Event::MatchList(event) => batch.match_list(event),
                    Event::Layout(event) => {
                        if event == LayoutEvent::TogglePreview {
                            batch.toggle_preview_enabled();
                        }
                        layout.handle(event, batch.preview_enabled());
                    }
                    Event::Preview(event) => batch.preview(event),
                    Event::Redraw => force_redraw = true,
                    Event::Quit => break 'selection Ok(Exit::Quit),
                    Event::QuitPromptEmpty => {
                        if batch.prompt_is_empty() {
                            break 'selection Ok(Exit::Quit);
                        }
                    }
                    Event::Select => {
                        if let Some(exit) = batch.selection_exit() {
                            break 'selection Ok(exit);
                        }
                    }
                    Event::Status { id } => handle_status = Some(id),
                    Event::Restart => match self.restart_notifier {
                        Some(ref notifier) => {
                            let injector = batch.restart();
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
            let preview_enabled = batch.preview_enabled();
            drop(batch);
            frame_start = Instant::now();
            frame_number = frame_number.wrapping_add(1);
            let background_frame =
                frame_number.is_multiple_of(self.background_frame_frequency.get() as u64);
            picker.update(match_budget);
            let mut frame = picker.prepare_frame(FrameOptions {
                deadline: frame_start + self.interval,
                background: background_frame,
                visible_components: layout.visible_components(preview_enabled),
                layout_changed: layout.layout_changed(),
                force_redraw,
            });
            if frame.needs_render() {
                frame.resize(layout.geometry(terminal.dimensions()?, preview_enabled));
                frame.draw(&mut **terminal, frame.redraw())?;
            }

            let changed = frame.changed();
            changed_since_status |= changed;
            terminal.end_frame(changed)?;

            // handle the status event but do not fail if the status notifier
            // is disconnected
            if let Some(id) = handle_status.take()
                && let Some(ref notifier) = self.status_notifier
            {
                let view = frame.view();
                let status = view.match_status();
                let _ = notifier.push(PickerStatus {
                    id,
                    query: view.query().to_owned(),
                    changed: std::mem::take(&mut changed_since_status),
                    frame_changed: changed,
                    rank: view.context().rank(),
                    item_count: status.item_count,
                    selected_item_count: view.queue_status().map_or(0, |queue| queue.count),
                    matched_item_count: status.matched_item_count,
                    dimensions: view.geometry().unwrap().dimensions,
                    matching: status.matching,
                    injecting: status.injecting,
                });
            }
            frame.complete();
        };

        let cleanup = terminal.cleanup();
        DropGuard::dismiss(terminal);
        cleanup?;

        // process and return the selection
        let exit = exit?;
        Ok(picker.finish(exit))
    }

    pub(crate) fn match_budget(&self) -> Duration {
        Duration::from_millis(2 * self.interval.as_millis() as u64 / 3)
    }
}
