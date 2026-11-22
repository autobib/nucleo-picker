//! # Interface generics
//!
//! This module implements interface generics. The interface describes the interface boundary
//! between the picker model and externally managed state that is closely coupled to the picker

use std::time::Instant;

use crate::{
    event::PreviewEvent,
    preview::Content,
    session::{Components, Context, Dimensions, Exit, ItemId, QueueEvent, QueueStatus},
};

/// Generics for the picker interface.
///
/// This trait constrains the type parameter `I` of the [`Session`](crate::session::Session) when used interactively.
/// Morally, the [`Interface`] defines *the behaviour of the interactive picker session*. This includes:
///
/// 1. Single-selection and multi-selection modes, which allows queuing selections in the
///    interactive interface and sets the return type (the `Output` type parameter).
/// 2. Preview generation pickers, which render item previews in a separate pane.
///
/// This trait essentially exists since the interaction between these modes and the picker model is
/// bi-directional. For example, adding an item to the item queue requires that the item queue
/// informs the picker whether the item was added or not, so the picker can move the hovered item.
/// While it is in principle possible to drive the picker model independently of an interface, the
/// interface provides a callback-based abstraction which allows handling of events for individual
/// components automatically.
///
/// See the [nucleo-picker-interface](https://docs.rs/nucleo-picker-interface) crate for some
/// built-in interfaces which you may find more convenient than implementing this trait yourself.
///
/// Read the [session documentation](crate::session) for an overview of how this trait fits into the
/// session API more generally.
///
/// Also see [`nucleo-picker`'s interface call
/// order documentation](https://docs.rs/nucleo-picker/latest/nucleo_picker/struct.Picker.html#interface-call-order)
/// to see documentation of the standard callback order.
pub trait Interface<T>: Sized {
    /// Arbitrary state which persists for exactly one session.
    type State;

    /// Whether this interface provides a preview pane.
    const PREVIEW: bool = false;

    /// The output of the picker after selection.
    type Output<'a>: IntoIterator<Item = &'a T, IntoIter: ExactSizeIterator>
    where
        T: 'a;

    /// Initialize the state after initial match-list updates.
    ///
    /// For the built-in runner, the provided [`Context`] is the
    /// same context as is passed to [`before_frame`](Self::before_frame) which
    /// occurs during initialization. However, this may not be the case for general runners.
    fn init(&mut self, cx: &Context<'_, T>) -> Self::State;

    /// Process a restart event.
    ///
    /// The provided [`Context`] corresponds to the *previous* [generation](crate::Generation) at the moment that the
    /// restart event was processed. The internal session state is preserved (i.e., there are no
    /// calls to [`Interface::init`] or [`Interface::on_end`]).
    fn on_restart(&mut self, _state: &mut Self::State, _cx: &Context<'_, T>) {}

    /// Perform cleanup at the end of the session.
    ///
    /// This takes place before converting output in [`finish`](Self::finish) and also before
    /// propagating errors.
    fn on_end(&mut self, _state: &mut Self::State) {}

    /// Returns if a given identifier is a queued item.
    fn is_queued(&self, _state: &Self::State, _id: ItemId) -> bool {
        false
    }

    /// The current number of queued items and the limit, if queueing is supported.
    ///
    /// `None` means queueing is unsupported by this interface. This is used to render the marker
    /// in the status line.
    fn queue_status(&self, _state: &Self::State) -> Option<QueueStatus> {
        None
    }

    /// Process an event which modifies the item queue, returning whether or not there were changes
    /// to the queue.
    ///
    /// The returned bool is used, for instance, to decide if the hovered item should change for
    /// events which toggle and move the cursor.
    fn on_queue(
        &mut self,
        _state: &mut Self::State,
        _event: QueueEvent<'_, T>,
        _cx: &Context<'_, T>,
    ) -> bool {
        false
    }

    /// Generate the output on completion.
    ///
    /// This method is called after [`on_end`](Self::on_end) when handling one of the standard [exit types](Exit),
    /// including selection or quit.
    fn finish<'items>(
        &mut self,
        state: Self::State,
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items>;

    /// Perform work before rendering the frame, after processing matches and navigation.
    ///
    /// This method is called on every frame, regardless of whether changes occurred or not. The
    /// resulting redraw mask influences the subsequent screen redraw.
    ///
    /// The `matches_changed` bool is initially `true` and subsequently is set to `true` if the
    /// underlying match list has changed or if there was a restart (including restarts even when
    /// there are no items). Changes to the hovered item will not set this flag.
    fn before_frame(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _matches_changed: bool,
        _deadline: Instant,
    ) -> Components {
        Components::NONE
    }

    /// Perform work after rendering the frame.
    ///
    /// Note that this hook is called when
    /// [`PreparedFrame::complete`](crate::session::PreparedFrame::complete) is called, but not when
    /// a prepared frame is dropped.
    fn after_frame(
        &mut self,
        _state: &mut Self::State,
        _cx: &Context<'_, T>,
        _deadline: Instant,
    ) -> Components {
        Components::NONE
    }

    /// Return the current preview pane.
    ///
    /// If the preview pane is [enabled](Self::PREVIEW), this is usually the rendered preview pane corresponding to the current item. If the preview pane is not enabled, this value is ignored and the implementation should just return [`Content::Empty`]. This is the default value.
    fn preview_content<'a>(&'a self, _state: &'a Self::State) -> Content<'a> {
        Content::Empty
    }

    /// Handle a preview event.
    ///
    /// The session forwards preview events whenever [`Self::PREVIEW`] is true, including
    /// while hidden. Hosts decide whether to filter user input; the built-in runner
    /// discards preview events while hidden. [`Context::active_components`] reports the
    /// current visibility independently of event delivery.
    /// The returned [`Components`] is a mask referring to the subset of the screen which requires
    /// redraw.
    fn preview_event(
        &mut self,
        _state: &mut Self::State,
        _event: PreviewEvent,
        _cx: &Context<'_, T>,
    ) -> Components {
        Components::NONE
    }

    /// Update the dimensions of the preview pane.
    ///
    /// These are the dimensions of the drawable area of the preview pane (including width and
    /// height 0 if the preview pane if the preview pane is very small), or `None` if the preview
    /// pane is hidden.
    ///
    /// This callback updates preview presentation. The host repaints after resizing; the
    /// session does not invalidate components merely because their dimensions change.
    /// Changes to other components must be reported through the interface's invalidation hooks.
    fn preview_resize(&mut self, _state: &mut Self::State, _size: Option<Dimensions>) {}
}
