//! The internal generic pick implementation

use std::{
    mem::DropGuard,
    num::NonZero,
    time::{Duration, Instant},
};

use crate::{
    Injector, Render, Terminal,
    error::PickError,
    event::{Event, EventSource, LayoutEvent, PickerStatus, RecvError},
    frame::{self, Layout, LayoutConfig},
    interface::Interface,
    observer::Notifier,
    preview::BoundaryChars,
    rect::Dimensions,
    session::{Components, Driver, Exit, FrameOptions},
    terminal::session,
};

#[derive(Clone, Copy)]
pub(crate) struct RunnerConfig {
    pub(crate) timing: FrameTiming,
    pub(crate) layout: LayoutConfig,
    pub(crate) preview_boundary_chars: BoundaryChars,
}

impl RunnerConfig {
    pub(crate) const fn new() -> Self {
        Self {
            timing: FrameTiming::new(),
            layout: LayoutConfig::new(),
            preview_boundary_chars: BoundaryChars::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FrameTiming {
    pub(crate) interval: Duration,
    pub(crate) background_frame_interval: Duration,
}

impl FrameTiming {
    pub(crate) const fn new() -> Self {
        Self {
            interval: Duration::from_millis(15),
            background_frame_interval: Duration::from_millis(60),
        }
    }

    pub(crate) fn match_budget(self) -> Duration {
        Duration::from_millis(2 * self.interval.as_millis() as u64 / 3)
    }

    fn background_frame_frequency(self) -> NonZero<usize> {
        let frame_nanos = self.interval.as_nanos();
        let frames = if frame_nanos == 0 {
            1
        } else {
            self.background_frame_interval
                .as_nanos()
                .div_ceil(frame_nanos)
                .max(1)
        };
        NonZero::new(usize::try_from(frames).unwrap_or(usize::MAX)).unwrap()
    }
}

pub(crate) fn pick_impl<'items, T, R, E, W, I>(
    driver: &'items mut Driver<T, R>,
    interface: &mut I,
    config: RunnerConfig,
    restart_notifier: Option<&Notifier<Injector<T, R>>>,
    status_notifier: Option<&Notifier<PickerStatus>>,
    mut event_source: E,
    writer: &mut W,
) -> Result<I::Output<'items>, PickError<E::AbortErr>>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
    E: EventSource,
    W: Terminal,
{
    let timing = config.timing;
    let match_budget = timing.match_budget();
    let background_frame_frequency = timing.background_frame_frequency();
    let mut frame_start = Instant::now();
    driver.update_matches(match_budget);
    let mut picker = driver.session(interface);
    let mut terminal = session(writer);
    terminal.init()?;

    // The initial full repaint includes all changes reported by before_frame.
    let mut layout = Layout::new(
        Dimensions::default(),
        config.layout.reversed,
        config.layout.preview_ratio,
        picker.visible_components(),
    );
    let mut frame = picker.prepare_frame(FrameOptions::new(frame_start + timing.interval));
    layout.resolve(terminal.dimensions()?, frame.visible_components());
    frame::draw(
        &layout,
        &mut frame,
        &mut **terminal,
        Components::ALL,
        true,
        config.preview_boundary_chars,
    )?;
    frame.complete();

    let mut frame_number = 0_u64;
    let mut handle_status = None;
    let mut changed_since_status = true;
    let exit = 'selection: loop {
        let mut render_now = false;
        let mut clear_screen = false;
        let frame_deadline = frame_start + timing.interval;
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
                    if render_now || now >= frame_deadline {
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
                    let visible = batch.visible_components();
                    let changed = match event {
                        LayoutEvent::ToggleStatus | LayoutEvent::TogglePreview => {
                            let mut components = visible;
                            if event == LayoutEvent::ToggleStatus {
                                components.status.toggle();
                            } else {
                                components.preview.toggle();
                            }
                            batch.set_visible_components(components);
                            let components = batch.visible_components();
                            if components != visible {
                                layout.resolve(layout.dimensions(), components);
                            }
                            components != visible
                        }
                        LayoutEvent::MoveDividerLeft(columns) => {
                            layout.shift_preview(i32::from(columns), visible)
                        }
                        LayoutEvent::MoveDividerRight(columns) => {
                            layout.shift_preview(-i32::from(columns), visible)
                        }
                    };
                    // Layout events request a repaint without ending the event batch early.
                    if changed {
                        clear_screen = true;
                    }
                }
                Event::Preview(event) => {
                    if batch.visible_components().preview {
                        batch.preview(event);
                    }
                }
                Event::Redraw => {
                    clear_screen = true;
                    render_now = true;
                }
                Event::Quit => break 'selection Ok(Exit::Quit),
                Event::QuitPromptEmpty => {
                    if batch.query().is_empty() {
                        break 'selection Ok(Exit::Quit);
                    }
                }
                Event::Select => {
                    let hovered = batch.hovered_id();
                    if hovered.is_some()
                        || batch.queue_status().is_some_and(|queue| queue.count != 0)
                    {
                        break 'selection Ok(Exit::Selected(hovered));
                    }
                }
                Event::Status { id } => handle_status = Some(id),
                Event::Restart => match restart_notifier {
                    Some(notifier) => {
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
        drop(batch);
        frame_start = Instant::now();
        frame_number = frame_number.wrapping_add(1);
        let background_frame = frame_number.is_multiple_of(background_frame_frequency.get() as u64);
        picker.update_matches(match_budget);
        let mut frame = picker.prepare_frame(
            FrameOptions::new(frame_start + timing.interval).background(background_frame),
        );
        if frame.changed() || clear_screen {
            clear_screen |= layout.resolve(terminal.dimensions()?, frame.visible_components());
            let redraw = frame.redraw();
            frame::draw(
                &layout,
                &mut frame,
                &mut **terminal,
                redraw,
                clear_screen,
                config.preview_boundary_chars,
            )?;
        }

        let changed = frame.changed() || clear_screen;
        changed_since_status |= changed;
        terminal.end_frame(changed)?;

        // handle the status event but do not fail if the status notifier
        // is disconnected
        if let Some(id) = handle_status.take()
            && let Some(notifier) = status_notifier
        {
            let status = frame.match_status();
            let _ = notifier.push(PickerStatus {
                id,
                query: frame.query().to_owned(),
                changed: std::mem::take(&mut changed_since_status),
                frame_changed: changed,
                rank: frame.rank(),
                item_count: status.item_count,
                selected_item_count: frame.queue_status().map_or(0, |queue| queue.count),
                matched_item_count: status.matched_item_count,
                dimensions: layout.dimensions(),
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
