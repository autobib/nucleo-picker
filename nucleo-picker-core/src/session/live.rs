mod match_list;
mod preview;
mod prompt;
mod read;
mod status;

pub use match_list::PreparedMatchList;
pub use preview::PreparedPreview;
pub use prompt::PreparedPrompt;
pub use status::PreparedStatus;

use read::SessionRead;
use std::{
    mem::DropGuard,
    time::{Duration, Instant},
};

use super::{Components, Context, Exit, ItemId, ItemRef, MatchStatus, Matches, QueueStatus};
use crate::{
    Generation, Injector, PickerChars, Render,
    component::Component,
    draw::Surface,
    event::{MatchListEvent, PreviewEvent, PromptEvent},
    interface::Interface,
    match_engine::MatchEngine,
    match_list::{MatchList, MatchListState, MatchListView},
    prompt::{Prompt, PromptState},
    rect::Dimensions,
    status_line::StatusLine,
};

type Guard<T> = DropGuard<T, fn(T)>;

trait Draw {
    fn resize(&mut self, size: Dimensions);
    fn draw<S: Surface>(&mut self, surface: &mut S) -> Result<(), S::Error>;
}

/// An externally-driven interactive picker session.
///
/// This is the main entry-point for externally driving a picker. Essentially, the session consists
/// of the reusable picker state combined with an interface.
///
/// ## Typical use
///
/// Initialize this struct with [`Model::session`](crate::session::Model::session) and use the
/// relevant methods to drive the session. In short, the frame loop typically looks something like
/// the following:
///
/// 1. Begin an event batch with [`Session::events`], and update the picker state with changes
///    coming from the user (such as key presses) or elsewhere.
/// 2. Update the match list using [`Session::update_matches`].
/// 3. Prepare a frame with [`Session::prepare_frame`]. Use the prepared frame for drawing to the
///    actual terminal. When rendering is complete, call [`PreparedFrame::complete`].
///
/// ## Exiting the session
///
/// The session terminates when the [`Session::finish`] method is called. The resulting output items
/// are determined by consuming the session state (in particular, the interface state iself) and
/// returning the corresponding output.
///
/// Ordinarily, dropping a session saves the state in the underlying [`Model`](crate::session::Model)
/// and calls [`Interface::on_end`], but does *not* call [`Interface::finish`]. The callback is
/// skipped when unwinding from a panic.
///
/// ## Redraw policy
///
/// Generally speaking, the runner is responsible for layout and redrawing. However, the various
/// components must generally speaking aware of their size. Therefore size changes are propagated to
/// components (specifically, the [`PreparedPrompt`], [`PreparedMatchList`], [`PreparedStatus`], and [`PreparedPreview`])
/// whenever the corresponding `resize` or `draw` methods are called.
///
/// When the session processes events (through an [`EventBatch`]), this may cause changes to the
/// internal components. Therefore, the session keeps track of a list of components which have
/// changed and reports them using [`PreparedFrame::redraw`] (see also [`PreparedFrame::changed`]).
///
/// Components can be drawn independently if a redraw is required or not. The state is mainly tracked
/// for those backends which may benefit from not redrawing on every frame. The redraw state is
/// reset when the frame is completed by calling [`PreparedFrame::complete`].
///
/// ## Driving the interface
///
/// Calls to various methods in the session (and sub-types) propagate calls to the underlying
/// interface as follows:
///
/// | Session operation | Interface operation |
/// | --- | --- |
/// | [`Model::session`](crate::session::Model::session) | Calls [`Interface::init`]. |
/// | [`Session::prepare_frame`] | Calls [`Interface::before_frame`] then [`Interface::queue_status`] to update the status line. |
/// | [`PreparedPreview::resize`] or [`PreparedPreview::draw`] | Calls [`Interface::preview_resize`] when preview dimensions or visibility change and [`Interface::PREVIEW`] is enabled. |
/// | [`PreparedFrame::complete`] | Calls [`Interface::after_frame`]. |
/// | [`Session::finish`] | Calls [`Interface::on_end`], then [`Interface::finish`] to produce the output. |
/// | Session drop | Calls [`Interface::on_end`] only (except during panic unwinding). |
///
/// Also see [`nucleo-picker`'s interface call order](https://docs.rs/nucleo-picker/latest/nucleo_picker/struct.Picker.html#interface-call-order)
/// for an example of how a runner might drive a session.
pub struct Session<'items, T: Send + Sync + 'static, R, I: Interface<T>> {
    // Dismissing the guard lets finish move the original storage borrow into the output context.
    inner: Guard<SessionState<'items, T, R, I>>,
}

struct SessionState<'items, T, R, I: Interface<T>> {
    engine: &'items mut MatchEngine<T, R>,
    // Drop state first so pending previews are cancelled before the interface joins workers,
    // including during panic unwinding when on_end is skipped.
    state: I::State,
    interface: I,
    prompt: Prompt<'items>,
    list: MatchList<'items>,
    status_line: StatusLine<'items>,
    chars: &'items PickerChars,
    status: MatchStatus,
    active_components: Components,
    query_changed: bool,
    matches_changed: bool,
    pending_redraw: Components,
    preview_resize: Option<Option<Dimensions>>,
}

/// Options concerning the presentation of the frame.
///
/// This struct contains information about *what the caller expects to do in this frame*. Most
/// importantly, this is used to communicate the frame processing deadline to the session.
///
/// The session also uses this information to elide certain operations and to influence presentation.
pub struct FrameOptions {
    deadline: Instant,
    background: bool,
}

impl FrameOptions {
    /// Set options for rendering the frame.
    ///
    /// The provided deadline is passed to the interface hooks when preparing the frame.
    #[must_use]
    #[inline]
    pub const fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            background: false,
        }
    }

    /// Set whether this is a background frame.
    ///
    /// The various components may use this for low-frequency updates (for instance, the spinner
    /// animation). The default value is false.
    #[must_use]
    #[inline]
    pub const fn background(mut self, background: bool) -> Self {
        self.background = background;
        self
    }
}

impl<'items, T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> Session<'items, T, R, I> {
    pub(crate) fn new(
        engine: &'items mut MatchEngine<T, R>,
        list_state: &'items mut MatchListState,
        prompt_state: &'items mut PromptState,
        chars: &'items PickerChars,
        mut interface: I,
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
        let status = engine.status();
        let active_components = Components {
            preview: I::PREVIEW,
            ..Components::ALL
        };
        let cx = context(engine, list.hovered_rank(engine), active_components);
        let state = interface.init(&cx);
        Self {
            inner: DropGuard::new(
                SessionState {
                    engine,
                    state,
                    interface,
                    prompt,
                    list,
                    status_line: StatusLine::new(chars),
                    chars,
                    status,
                    active_components,
                    query_changed: false,
                    matches_changed: true,
                    pending_redraw: Components::ALL,
                    preview_resize: None,
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

    /// Begin an event batch.
    ///
    /// The event batch is used to change the state of the picker, mainly by processing events.
    /// Read the [`EventBatch`] documentation for more details.
    pub fn events(&mut self) -> EventBatch<'_, 'items, T, R, I> {
        EventBatch {
            session: &mut self.inner,
        }
    }

    /// Update the internal match engine and match list after an event batch, returning the status
    /// of the match list.
    ///
    /// The suggested wait time is used to acquire a lock on the background match worker thread
    /// pool. A wait time of 0, while allowed, may result in the lock not being acquired and the
    /// event list not being updated. It is recommend to set this to at least a few milliseconds.
    pub fn update_matches(&mut self, wait: Duration) -> MatchStatus {
        let session = &mut *self.inner;
        session.status = session.engine.update(wait_millis(wait));
        session.matches_changed |= session.status.items_changed;
        session.pending_redraw.match_list |= session.list.update(
            session.engine,
            session.status.items_changed,
            std::mem::take(&mut session.query_changed),
        );
        session.status
    }

    /// Prepare a frame for rendering.
    ///
    /// This runs the [`Interface::before_frame`] status hook and propagates state between changes
    /// between components.
    ///
    /// The returned [`PreparedFrame`] can be used for drawing the frame and introspecting the
    /// state.
    pub fn prepare_frame(&mut self, options: FrameOptions) -> PreparedFrame<'_, 'items, T, R, I> {
        let session = &mut *self.inner;
        let cx = context(
            session.engine,
            session.list.hovered_rank(session.engine),
            session.active_components,
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
        PreparedFrame {
            session,
            deadline: options.deadline,
            completed: false,
        }
    }

    /// Complete the session, returning the final output items.
    pub fn finish(self, exit: Exit) -> I::Output<'items> {
        let mut session = DropGuard::dismiss(self.inner);
        session.commit();
        session.interface.on_end(&mut session.state);
        let cx = context(
            session.engine,
            session.list.hovered_rank(session.engine),
            session.active_components,
        );
        session.interface.finish(session.state, exit, cx)
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionRead for Session<'_, T, R, I> {
    type Item = T;
    type Renderer = R;
    type Interface = I;

    fn session(&self) -> &SessionState<'_, T, R, I> {
        &self.inner
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> Session<'_, T, R, I> {
    /// Returns the generation of the underlying item pool.
    #[must_use]
    #[inline]
    pub fn generation(&self) -> Generation {
        SessionRead::generation(self)
    }

    /// Returns the complete contents of the query string (independently of what is visible).
    pub fn query(&self) -> &str {
        SessionRead::query(self)
    }

    /// Returns a view of the current match snapshot in match-rank order.
    pub fn matches(&self) -> Matches<'_, T> {
        SessionRead::matches(self)
    }

    /// Returns the match rank of the hovered item, or `None` if there are no matches.
    pub fn hovered_rank(&self) -> Option<u32> {
        SessionRead::hovered_rank(self)
    }

    /// Returns the item at the committed hovered rank, or `None` if there are no matches.
    pub fn hovered(&self) -> Option<ItemRef<'_, T>> {
        SessionRead::hovered(self)
    }

    /// Returns the set of active components, as understood by the session.
    ///
    /// The active component list is used internally to avoid unnecessary background work.
    /// This value is only updated using [`EventBatch::set_active_components`].
    pub fn active_components(&self) -> Components {
        SessionRead::active_components(self)
    }

    /// Returns the current status of matching progress.
    ///
    /// This only returns the status from the last call to [`update_matches`](Session::update_matches), and does not provide any internal matching updates.
    pub fn match_status(&self) -> MatchStatus {
        SessionRead::match_status(self)
    }

    /// Returns the current status of the item queue.
    ///
    /// Note that [`None`] means that the interface does not support queueing at all.
    pub fn queue_status(&self) -> Option<QueueStatus> {
        SessionRead::queue_status(self)
    }

    /// Access the internal interface and its state.
    pub fn interface(&self) -> (&I, &I::State) {
        (&self.inner.interface, &self.inner.state)
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
        self.pending_redraw.match_list |= self.list.update(self.engine, false, false);
    }
}

/// A handle to process a batch of events.
///
/// This handle is used to propagate events to the session. The session batches events together
/// before processing them in order to reduce overhead. When this handle is dropped, the changes
/// from events are recorded in the underlying session. Reading [`Self::query`] also applies
/// buffered changes while continuing to use the batch.
///
/// The event batch has an internal event queue which helps with event deduplication. Dropping the
/// event batch causes the internal event queue to be published to the original [`Session`]. Note
/// that any changes to the event batch are observable by future calls: for instance updates to the
/// query in the event batch are immediately visible in the call to [`query`](Self::query).
pub struct EventBatch<'batch, 'items, T: Send + Sync + 'static, R, I: Interface<T>> {
    session: &'batch mut SessionState<'items, T, R, I>,
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> EventBatch<'_, '_, T, R, I> {
    /// Returns the current [`Generation`].
    ///
    /// This takes into account an earlier restart processed in the same batch.
    #[must_use]
    #[inline]
    pub fn generation(&self) -> Generation {
        self.session.engine.generation()
    }

    /// Send an event which updates the prompt to the picker.
    pub fn prompt(&mut self, event: PromptEvent) {
        self.session.prompt.handle(event);
    }

    /// Send an event which updates the match list to the picker.
    pub fn match_list(&mut self, event: MatchListEvent) {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.event_rank(session.engine),
            session.active_components,
        );
        session
            .list
            .handle(event, &mut session.interface, &mut session.state, &cx);
    }

    /// Send an event which updates the preview pane to the picker.
    ///
    /// Events are forwarded whenever [`Interface::PREVIEW`] is true, including while hidden.
    /// The interface receives the event-time hovered item and preview visibility, and uses
    /// the last dimensions delivered to it. Hosts decide whether to filter user input while
    /// the preview is hidden.
    pub fn preview(&mut self, event: PreviewEvent) {
        let session = &mut *self.session;
        if I::PREVIEW {
            let cx = context(
                session.engine,
                session.list.event_rank(session.engine),
                session.active_components,
            );
            session.pending_redraw |=
                session
                    .interface
                    .preview_event(&mut session.state, event, &cx);
        }
    }

    /// Returns the identifier of the hovered item, or `None` if there are no matches.
    ///
    /// This method is often used for terminating a session by checking if an identifier is present
    /// (or if the item queue is non-empty), and then dropping the batch and passing the identifier
    /// to [`Session::finish`] within an [`Exit::Selected`].
    pub fn hovered_id(&self) -> Option<ItemId> {
        let session = &*self.session;
        session
            .list
            .event_rank(session.engine)
            .map(|rank| session.engine.item_id(session.engine.idx_from_match(rank)))
    }

    /// Returns the interface's queue status, including changes from earlier events in this batch.
    ///
    /// `None` means queueing is unsupported, rather than that the queue is empty.
    /// Reading does not update matching or run frame hooks.
    pub fn queue_status(&self) -> Option<QueueStatus> {
        self.session.interface.queue_status(&self.session.state)
    }

    /// Apply buffered prompt and match-list edits and return the complete query string.
    ///
    /// This includes all preceding prompt events in the batch without advancing matching or
    /// running frame hooks. Further events can be sent after reading the query.
    pub fn query(&mut self) -> &str {
        self.flush();
        self.session.prompt.contents()
    }

    fn flush(&mut self) {
        self.session.commit();
    }

    /// Restart the picker, discarding the existing items and returning a new injector valid for the new
    /// item pool.
    ///
    /// Note that this does not clear the query string. This updates the [generation](Generation).
    pub fn restart(&mut self) -> Injector<T, R> {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.event_rank(session.engine),
            session.active_components,
        );
        session.interface.on_restart(&mut session.state, &cx);
        session.pending_redraw |= Components::QUEUE | Components::PREVIEW;
        session.engine.restart();
        session.matches_changed = true;
        session
            .list
            .restart(&context(session.engine, None, session.active_components));
        session.engine.injector()
    }

    /// Returns the current list of active components.
    ///
    /// See [`Session::active_components`] for more details.
    pub fn active_components(&self) -> Components {
        self.session.active_components
    }

    /// Set the list of active components.
    ///
    /// Note that preview visibility also takes into account [`Interface::PREVIEW`]. If
    /// [`Interface::PREVIEW`] is set to false, this means that the preview pane is never active
    /// (since it is the responsibility of the interface only to manage the preview pane) so the
    /// component will be inactive independently of the value set here.
    pub fn set_active_components(&mut self, mut components: Components) {
        components.preview &= I::PREVIEW;
        let visible = self.session.active_components;
        self.session.pending_redraw |= Components {
            prompt: visible.prompt != components.prompt,
            match_list: visible.match_list != components.match_list,
            status: visible.status != components.status,
            preview: visible.preview != components.preview,
        };
        self.session.active_components = components;
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> Drop for EventBatch<'_, '_, T, R, I> {
    fn drop(&mut self) {
        self.flush();
    }
}

/// A representation of a single frame in the session.
///
/// One can think of this type as a snapshot of the prepared frame. It has sub-types for the
/// different components:
///
/// - [`prompt`](Self::prompt), returning [`PreparedPrompt`].
/// - [`match_list`](Self::match_list), returning [`PreparedMatchList`].
/// - [`status`](Self::status), returning [`PreparedStatus`].
/// - [`preview`](Self::preview), returning [`PreparedPreview`].
///
/// Each component contains methods for introspection, resizing, and drawing. If in addition the
/// `ratatui` feature is enabled the component also implements
/// [`Widget`](ratatui_core::widgets::Widget).
///
/// When the frame is complete, call [`complete`](Self::complete) to propagate context
/// to the [`Interface`] struct and reset internal state. Dropping a prepared frame without calling
/// [`complete`](Self::complete) will skip [`Interface::after_frame`] and set all active
/// components as having been modified.
pub struct PreparedFrame<'frame, 'items, T, R, I: Interface<T>> {
    session: &'frame mut SessionState<'items, T, R, I>,
    deadline: Instant,
    completed: bool,
}

impl<'items, T: Send + Sync + 'static, R: Render<T>, I: Interface<T>>
    PreparedFrame<'_, 'items, T, R, I>
{
    /// Returns a view of the prompt.
    ///
    /// This view can be used for resizing, drawing, and introspection. See the [`PreparedPrompt`]
    /// documentation for more details.
    pub fn prompt(&mut self) -> PreparedPrompt<'_, 'items, T, R, I> {
        PreparedPrompt {
            session: self.session,
        }
    }

    /// Returns a view of the match list.
    ///
    /// This view can be used for resizing, drawing, and introspection. See the [`PreparedMatchList`]
    /// documentation for more details.
    pub fn match_list(&mut self) -> PreparedMatchList<'_, 'items, T, R, I> {
        PreparedMatchList {
            session: self.session,
        }
    }

    /// Returns a view of the status bar.
    ///
    /// This view can be used for resizing, drawing, and introspection. See the [`PreparedStatus`]
    /// documentation for more details.
    pub fn status(&mut self) -> PreparedStatus<'_, 'items, T, R, I> {
        PreparedStatus {
            session: self.session,
        }
    }

    /// Returns a view of the preview pane.
    ///
    /// This view can be used for resizing, drawing, and introspection. See the [`PreparedPreview`]
    /// documentation for more details.
    pub fn preview(&mut self) -> PreparedPreview<'_, 'items, T, R, I> {
        PreparedPreview {
            session: self.session,
            border: None,
        }
    }

    /// Returns the visible components invalidated by session changes.
    ///
    /// The redraw list is filtered by the visible components (as returned by
    /// [`PreparedFrame::active_components`] and set by
    /// [`EventBatch::set_active_components`]). In particular, the redraw mask may be empty and
    /// [`changed`](Self::changed) can return true if a component goes from being visible to hidden.
    /// The host must also account for layout and presentation changes, which are not tracked here.
    pub fn redraw(&self) -> Components {
        self.session.pending_redraw & self.session.active_components
    }

    /// Returns whether any of the components have changed.
    ///
    /// This includes changes reported by [`redraw`](Self::redraw) and also changes to visibility
    /// (for instance, if a component is hidden or becomes visible).
    /// Host-driven resizing, drawing, and presentation changes do not affect this value, so
    /// `false` does not imply that the host can skip rendering.
    pub fn changed(&self) -> bool {
        !self.session.pending_redraw.is_none()
    }

    /// Acknowledge completion of the current frame.
    ///
    /// Calling this method resets the internal redraw markers and calls [`Interface::after_frame`].
    /// Any resulting redraw information from [`Interface::after_frame`] will be stored internally
    /// for use on the next frame.
    ///
    /// This method should be called on every frame, including frames which did not require rendering.
    pub fn complete(mut self) {
        let session = &mut *self.session;
        let cx = context(
            session.engine,
            session.list.hovered_rank(session.engine),
            session.active_components,
        );
        session.pending_redraw = Components::NONE;
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
            self.session.pending_redraw = Components::ALL;
        }
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> SessionRead for PreparedFrame<'_, '_, T, R, I> {
    type Item = T;
    type Renderer = R;
    type Interface = I;

    fn session(&self) -> &SessionState<'_, T, R, I> {
        self.session
    }
}

impl<T: Send + Sync + 'static, R, I: Interface<T>> PreparedFrame<'_, '_, T, R, I> {
    /// Returns the generation of the underlying item pool.
    #[must_use]
    #[inline]
    pub fn generation(&self) -> Generation {
        SessionRead::generation(self)
    }

    /// Returns the complete contents of the query string (independently of what is visible).
    pub fn query(&self) -> &str {
        SessionRead::query(self)
    }

    /// Returns a view of the current match snapshot in match-rank order.
    ///
    /// The view borrows this frame, independently of component sizing and visibility.
    /// Reading does not advance matching or run frame hooks.
    pub fn matches(&self) -> Matches<'_, T> {
        SessionRead::matches(self)
    }

    /// Returns the match rank of the hovered item, or `None` if there are no matches.
    pub fn hovered_rank(&self) -> Option<u32> {
        SessionRead::hovered_rank(self)
    }

    /// Returns the item at the committed hovered rank, or `None` if there are no matches.
    ///
    /// The item borrows this frame and comes from [`Self::matches`] at [`Self::hovered_rank`],
    /// independently of component sizing and visibility. Reading does not advance matching
    /// or run frame hooks.
    pub fn hovered(&self) -> Option<ItemRef<'_, T>> {
        SessionRead::hovered(self)
    }

    /// Returns the set of components which are visible.
    ///
    /// This set is used to decide *how to allocate layout* rather than for redraw. Note that
    /// components allocated an empty area in the screen can still count as visible.
    ///
    /// Manipulate the set of visible components using [`EventBatch::set_active_components`].
    pub fn active_components(&self) -> Components {
        SessionRead::active_components(self)
    }

    /// Returns the current status of matching progress.
    ///
    /// This only returns the status from the last call to [`update_matches`](Session::update_matches), and does not provide any internal matching updates.
    pub fn match_status(&self) -> MatchStatus {
        SessionRead::match_status(self)
    }

    /// Returns the current status of the item queue.
    ///
    /// Note that [`None`] means that the interface does not support queueing at all.
    pub fn queue_status(&self) -> Option<QueueStatus> {
        SessionRead::queue_status(self)
    }
}

impl<T: Send + Sync + 'static, R: Render<T>, I: Interface<T>> SessionState<'_, T, R, I> {
    fn list(&mut self) -> impl Component {
        MatchListView {
            list: &mut self.list,
            engine: self.engine,
            interface: &self.interface,
            state: &self.state,
        }
    }
}

fn dimensions<S: Surface>(surface: &S) -> Dimensions {
    Dimensions {
        width: surface.width(),
        height: surface.height(),
    }
}

fn context<T: Send + Sync + 'static, R>(
    engine: &MatchEngine<T, R>,
    rank: Option<u32>,
    active_components: Components,
) -> Context<'_, T> {
    Context::new(Matches::new(engine))
        .with_rank(rank)
        .with_active_components(active_components)
}

fn wait_millis(wait: Duration) -> u64 {
    u64::try_from(wait.as_millis()).unwrap_or(u64::MAX)
}
