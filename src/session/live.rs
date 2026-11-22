use std::{
    io,
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Context, Exit, MatchStatus, Matches, QueueStatus, Redraw};
use crate::{
    Injector, PickerChars, Render, Terminal,
    component::Component,
    event::{MatchListEvent, PromptEvent},
    frame::Contents,
    interface::Interface,
    match_engine::MatchEngine,
    match_list::{MatchList, MatchListState, MatchListView},
    preview::{PreviewConfig, PreviewEvent},
    prompt::{Prompt, PromptState},
    rect::{Area, Dimensions},
    status_line::StatusLine,
};

type Guard<T> = DropGuard<T, fn(T)>;

/// An externally-driven interactive picker session.
// TODO: add all of the relevant docs, esp. batch/update/prepare/complete lifecycle and normal abandonment
pub(crate) struct Session<'items, T: Send + Sync + 'static, R, I: Interface<T>> {
    // Dismissing the guard lets finish move the original storage borrow into the output context.
    inner: Guard<SessionState<'items, T, R, I>>,
}

struct SessionState<'items, T, R, I: Interface<T>> {
    engine: &'items mut MatchEngine<T, R>,
    interface: I,
    state: I::State,
    prompt: Prompt<'items>,
    list: MatchList<'items>,
    status_line: StatusLine<'items>,
    chars: &'items PickerChars,
    preview_config: &'items PreviewConfig,
    status: MatchStatus,
    preview_enabled: bool,
    query_changed: bool,
    matches_changed: bool,
    pending_redraw: Redraw,
    pending_resize: bool,
    full_redraw: bool,
    geometry: Option<Geometry>,
}

/// Options concerning the presentation of the frame.
///
/// This struct contains information about *what the caller expects to do in this frame*. Most
/// importantly, this is used to communicate the frame processing deadline to the session.
///
/// The session also uses this information to elide certain operations and to influence presentation.
/// For example, if the preview pane is hidden, the session will skip update checks.
pub(crate) struct FrameOptions {
    pub deadline: Instant,
    pub background: bool,
    pub visible_components: Redraw,
    pub layout_changed: bool,
    pub force_redraw: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Geometry {
    pub dimensions: Dimensions,
    pub prompt: Area,
    pub list: Area,
    pub status: Area,
    pub preview: Area,
}

impl<'items, T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Session<'items, T, R, I> {
    pub(crate) fn new(
        engine: &'items mut MatchEngine<T, R>,
        list_state: &'items mut MatchListState,
        prompt_state: &'items mut PromptState,
        chars: &'items PickerChars,
        preview_config: &'items PreviewConfig,
        mut interface: I,
        startup_wait: Duration,
    ) -> Self {
        // Headless updates defer layout work until the next interactive session.
        if std::mem::take(&mut list_state.items_changed) {
            list_state
                .layout
                .update_items(engine.snapshot(), &list_state.config);
        }
        let prompt = Prompt::new(prompt_state, chars);
        let mut list = MatchList::new(list_state, chars);
        list.flush(engine);
        let status = engine.update(wait_millis(startup_wait));
        list.update(engine, status.items_changed, false);
        let cx = context(engine, list.rank(engine), I::PREVIEW);
        let state = interface.init(&cx);
        Self {
            inner: DropGuard::new(
                SessionState {
                    engine,
                    interface,
                    state,
                    prompt,
                    list,
                    status_line: StatusLine::new(chars),
                    chars,
                    preview_config,
                    status,
                    preview_enabled: I::PREVIEW,
                    query_changed: false,
                    matches_changed: true,
                    pending_redraw: Redraw::FULL,
                    pending_resize: true,
                    full_redraw: true,
                    geometry: None,
                },
                |mut session| {
                    session.commit();
                    if !std::thread::panicking() {
                        session.interface.on_end(&mut session.state);
                    }
                },
            ),
        }
    }

    pub fn events(&mut self) -> EventBatch<'_, 'items, T, R, I> {
        EventBatch {
            session: &mut self.inner,
        }
    }

    pub fn update(&mut self, wait: Duration) -> MatchStatus {
        let session = &mut *self.inner;
        session.status = session.engine.update(wait_millis(wait));
        session.matches_changed |= session.status.items_changed;
        session.pending_redraw.list |= session.list.update(
            session.engine,
            session.status.items_changed,
            std::mem::take(&mut session.query_changed),
        );
        session.status
    }

    pub fn prepare_frame(&mut self, options: FrameOptions) -> PreparedFrame<'_, 'items, T, R, I> {
        let session = &mut *self.inner;
        let cx = context(
            session.engine,
            session.list.rank(session.engine),
            session.preview_enabled,
        );
        session.pending_redraw |= session.interface.before_frame(
            &mut session.state,
            &cx,
            std::mem::take(&mut session.matches_changed),
            options.deadline,
        );
        session.pending_redraw.status |= session.status_line.update(
            session.status,
            session.interface.queue_status(&session.state),
            options.background,
        );
        session.pending_resize |= options.layout_changed;
        session.full_redraw |= options.force_redraw || session.pending_resize;
        let mut visible = options.visible_components;
        visible.preview &= session.preview_enabled;
        PreparedFrame {
            session,
            visible,
            deadline: options.deadline,
            completed: false,
        }
    }

    pub fn finish(self, exit: Exit) -> I::Output<'items> {
        let mut session = DropGuard::dismiss(self.inner);
        session.commit();
        session.interface.on_end(&mut session.state);
        let cx = context(
            session.engine,
            session.list.rank(session.engine),
            session.preview_enabled,
        );
        session.interface.finish(session.state, exit, cx)
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionState<'_, T, R, I> {
    fn commit(&mut self) {
        self.list.flush(self.engine);
        let prompt = self.prompt.update();
        if prompt.contents_changed {
            self.engine.reparse(self.prompt.contents());
        }
        self.query_changed |= prompt.contents_changed;
        self.pending_redraw.prompt |= prompt.needs_redraw;
        self.pending_redraw.list |= self.list.update(self.engine, false, false);
    }
}

/// A handle to process a batch of events.
///
/// This handle is used to propagate events to the session. The session batches events together
/// before processing them in order to reduce overhead. When this handle is dropped, the changes
/// from events are recorded in the underlying session.
pub(crate) struct EventBatch<'batch, 'items, T: Send + Sync + 'static, R, I: Interface<T>> {
    session: &'batch mut SessionState<'items, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> EventBatch<'_, '_, T, R, I> {
    pub fn prompt(&mut self, event: PromptEvent) {
        self.session.prompt.handle(event);
    }

    pub fn match_list(&mut self, event: MatchListEvent) {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.event_rank(session.engine),
            session.preview_enabled,
        );
        session
            .list
            .handle(event, &mut session.interface, &mut session.state, &cx);
    }

    pub fn preview(&mut self, event: PreviewEvent) {
        let session = &mut *self.session;
        if session.preview_enabled {
            let cx = context(
                session.engine,
                session.list.event_rank(session.engine),
                true,
            );
            session.pending_redraw |=
                session
                    .interface
                    .preview_event(&mut session.state, event, &cx);
        }
    }

    // TODO: Document event-time selection; hosts must stop dispatch and matching after choosing to exit.
    pub fn selection_exit(&self) -> Option<Exit> {
        let session = &*self.session;
        let hovered = session
            .list
            .event_rank(session.engine)
            .map(|rank| session.engine.item_id(session.engine.idx_from_match(rank)));
        (hovered.is_some()
            || session
                .interface
                .queue_status(&session.state)
                .is_some_and(|queue| queue.count != 0))
        .then_some(Exit::Selected(hovered))
    }

    pub fn prompt_is_empty(&mut self) -> bool {
        self.session.prompt.flush();
        self.session.prompt.contents().is_empty()
    }

    pub fn restart(&mut self) -> Injector<T, R> {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.event_rank(session.engine),
            session.preview_enabled,
        );
        session.interface.on_restart(&mut session.state, &cx);
        session.pending_redraw |= Redraw::QUEUE | Redraw::PREVIEW;
        session.engine.restart();
        session.matches_changed = true;
        session
            .list
            .restart(&context(session.engine, None, session.preview_enabled));
        session.engine.injector()
    }

    pub fn preview_enabled(&self) -> bool {
        self.session.preview_enabled
    }

    pub fn set_preview_enabled(&mut self, enabled: bool) {
        let enabled = enabled && I::PREVIEW;
        if self.session.preview_enabled != enabled {
            self.session.preview_enabled = enabled;
            self.session.pending_resize = true;
        }
    }

    pub fn toggle_preview_enabled(&mut self) {
        self.set_preview_enabled(!self.preview_enabled());
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> Drop for EventBatch<'_, '_, T, R, I> {
    fn drop(&mut self) {
        self.session.commit();
    }
}

// TODO: Document explicit successful completion, status-before-completion ordering, and full repaint after abandonment.
pub(crate) struct PreparedFrame<'frame, 'items, T, R, I: Interface<T>> {
    session: &'frame mut SessionState<'items, T, R, I>,
    visible: Redraw,
    deadline: Instant,
    completed: bool,
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> PreparedFrame<'_, '_, T, R, I> {
    pub fn needs_render(&self) -> bool {
        self.changed()
    }

    pub fn redraw(&self) -> Redraw {
        let redraw = if self.session.full_redraw {
            Redraw::FULL
        } else {
            self.session.pending_redraw
        };
        redraw & self.visible
    }

    pub fn changed(&self) -> bool {
        self.session.full_redraw || self.redraw() != Redraw::NONE
    }

    pub fn resize(&mut self, geometry: Geometry) {
        let session = &mut *self.session;
        if session.geometry != Some(geometry) || session.pending_resize {
            session.prompt().resize(geometry.prompt);
            session.list().resize(geometry.list);
            session.status().resize(geometry.status);
            session.preview_resize(session.preview_enabled.then_some(geometry.preview));
            session.geometry = Some(geometry);
            session.pending_resize = false;
            session.full_redraw = true;
        }
    }

    pub fn view(&self) -> FrameView<'_, T, R, I> {
        FrameView {
            session: self.session,
        }
    }

    pub fn draw<W: Terminal>(&mut self, writer: &mut W, repaint: Redraw) -> io::Result<()> {
        let geometry = self.session.geometry.unwrap_or_default();
        let repaint = if self.session.full_redraw {
            Redraw::FULL
        } else {
            repaint
        };
        crate::frame::draw(geometry, self.session, writer, repaint, self.visible)
    }

    pub fn complete(mut self) {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.rank(session.engine),
            session.preview_enabled,
        );
        session.pending_redraw = Redraw::NONE;
        session.full_redraw = false;
        session.pending_redraw |=
            session
                .interface
                .after_frame(&mut session.state, &cx, self.deadline);
        self.completed = true;
    }
}

impl<T, R, I: Interface<T>> Drop for PreparedFrame<'_, '_, T, R, I> {
    fn drop(&mut self) {
        if !self.completed {
            self.session.full_redraw = true;
        }
    }
}

pub(crate) struct FrameView<'a, T, R, I: Interface<T>> {
    session: &'a SessionState<'a, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> FrameView<'_, T, R, I> {
    pub fn query(&self) -> &str {
        self.session.prompt.contents()
    }

    pub fn context(&self) -> Context<'_, T> {
        context(
            self.session.engine,
            self.session.list.rank(self.session.engine),
            self.session.preview_enabled,
        )
    }

    pub fn match_status(&self) -> MatchStatus {
        self.session.status
    }

    pub fn queue_status(&self) -> Option<QueueStatus> {
        self.session.interface.queue_status(&self.session.state)
    }

    pub fn geometry(&self) -> Option<Geometry> {
        self.session.geometry
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Contents
    for SessionState<'_, T, R, I>
{
    fn prompt(&mut self) -> impl Component {
        &mut self.prompt
    }

    fn list(&mut self) -> impl Component {
        MatchListView {
            list: &mut self.list,
            engine: self.engine,
            interface: &self.interface,
            state: &self.state,
        }
    }

    fn status(&mut self) -> impl Component {
        &mut self.status_line
    }

    fn preview(&mut self) -> impl Component {
        crate::component::PreviewPane {
            interface: &self.interface,
            state: &self.state,
            boundary_chars: self.preview_config.boundary_chars,
            ellipsis: self.chars.ellipsis,
        }
    }

    fn preview_resize(&mut self, area: Option<Area>) {
        if I::PREVIEW {
            self.interface.preview_resize(
                &mut self.state,
                area.map(crate::preview::interior_dimensions),
            );
        }
    }
}

fn context<T: Send + Sync + 'static, R>(
    engine: &MatchEngine<T, R>,
    rank: Option<u32>,
    preview_enabled: bool,
) -> Context<'_, T> {
    Context::new(Matches::new(engine))
        .with_rank(rank)
        .with_preview_enabled(preview_enabled)
}

fn wait_millis(wait: Duration) -> u64 {
    u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)
}
