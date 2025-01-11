//! # Event handling
//!
//! This module defines the core [`Event`] type handled by a [`Picker`](crate::Picker), which
//! defines an interactive update to the picker state.
//!
//! By default, the interactive picker launched by [`Picker::pick`](crate::Picker::pick) watches
//! for terminal events (such as key presses) and maps them to [`Event`]s. The process of reading
//! events is encapsulated in the [`EventSource`] trait, which you can implement yourself and pass
//! directly to the picker using the [`Picker::pick_with_io`](crate::Picker::pick_with_io).
//!
//! Jump to:
//! - The [`EventSource`] trait.
//! - The [`StdinReader`], for automatically reading events from standard input, with customizable
//!   keybindings.
//! - The [`StdinEventSender`] to read events from standard input and send them through a
//!   [mpsc channel](std::sync::mpsc::channel).
//! - The [default keybindings](keybind_default), which are also useful to provide fallbacks for
//!   keybind customization
//!
//! For somewhat comprehensive examples, see the [extended fzf
//! example](https://github.com/autobib/nucleo-picker/blob/master/examples/fzf_err_handling.rs) or
//! the [restart
//! example](https://github.com/autobib/nucleo-picker/blob/master/examples/restart.rs).

mod bind;

use std::{
    convert::Infallible,
    io,
    marker::PhantomData,
    sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender},
    time::Duration,
};

use crossterm::event::{poll, read, KeyEvent};

use self::bind::convert_crossterm_event;

pub use self::bind::keybind_default;
use crate::Injector;
pub use crate::{match_list::MatchListEvent, prompt::PromptEvent};

/// An event which controls the picker behaviour.
///
/// The type parameters `T` and `R` are the item type and the [`Render`](crate::Render)
/// implementation specific to the picker respectively. The type parameter `A` is the
/// application-defined error which can be used to conveniently propagate application errors
/// independent of the picker to the main thread where the picker is running.
///
/// Most events are explained directly in the enum variant documentation. A few special cases
/// require a bit more detail: [redraw](#redraw),
/// [application-defined abort](#application-defined-abort), and [restart](#restart)
///
/// ## Redraw
/// In most cases, it is not necessary to manually send an [`Event::Redraw`] since the default
/// behaviour of the picker is to automatically redraw on each frame if the state of the screen
/// would change when handling an event, or when the item list is updated internally.
///
/// There is no `Resize` variant since the screen size is automatically checked immediately before
/// drawing to the screen. If you are generating your own events, propagate a screen resize as a
/// [`Event::Redraw`], which will force a redraw to respect the new screen size.
///
/// ## Application-defined abort
/// The abort event is a special event used to propagate errors from the application to the picker.
/// When the picker receives an abort event, it immediately terminates and passes the abort event
/// onwards inside the [`PickError::Aborted`](crate::error::PickError::Aborted) error variant.
///
/// By default, the associated type parameter is `!`, which means that [`Event::Abort`] cannot be
/// constructed in ordinary circumstances. In order to generate [`Event::Abort`], you must use the
/// [`Picker::pick_with_io`](crate::Picker::pick_with_io) method and pass an appropriate
/// [`EventSource`] which generates your desired errors.
///
/// The provided [`EventSource`] implementations, namely [`StdinReader`] and
/// [`mpsc::Receiver`](std::sync::mpsc::Receiver), are both generic over the same type parameter
/// `A` so you can construct this variant with a custom error type if desired.
///
/// ## Restart
/// The [`Event::Restart`] is used to restart the picker while it is still running. After a
/// restart, all previously created [`Injector`]s become invalidated. Therefore to receive a valid
/// [`Injector`], the caller must provide the [`SyncSender`] end of a channel created by
/// [`mpsc::sync_channel`](std::sync::mpsc::sync_channel).
///
/// When the [`Event::Restart`] is processed by the picker, it will clear the item list and
/// immediately block to send the new injector through the channel. If the send fails because the
/// receiver end has been dropped, the picker will fail with
/// [`PickError::Disconnected`](crate::error::PickError::Disconnected).
///
/// At most one [`Injector`] will be sent down the channel each time the picker receives an
/// [`Event::Restart`]. It is possible that no [`Injector`] will be sent if the picker exists
/// or disconnects before the event is processed.
///
/// If you are processing the restart events in the same thread in which the events are generated,
/// you can use a channel with a size of 0 as long as you immediately call
/// [`Receiver::recv`](std::sync::mpsc::Receiver::recv) on the corresponding receiver end so that
/// the corresponding 'send' call does not block and reduce interactivity of the picker interface
/// unnecessarily. Otherwise, using an appropriate buffer size (tuned to the requirements of your
/// application) is recommended.
///
/// Using a very large buffer size is somewhat of an anti-pattern since the buffer queue
/// being completely filled means that the restart events are far outdated anyway. For
/// restart-intensive applications in which generation of items on each restart is slow, the item
/// source should periodically check the channel for new restart events and intterupt current
/// processing to prioritize any new restarts.
///
/// For a detailed implementation example, see the [restart
/// example](https://github.com/autobib/nucleo-picker/blob/master/examples/restart.rs).
#[non_exhaustive]
pub enum Event<T, R, A = Infallible> {
    /// Modify the prompt.
    Prompt(PromptEvent),
    /// Modify the list of matches.
    MatchList(MatchListEvent),
    /// Quit the picker (no selection).
    Quit,
    /// Quit the picker (no selection) if the prompt is empty.
    QuitPromptEmpty,
    /// Abort the picker (error) at user request.
    UserInterrupt,
    /// Abort the picker (error) for another reason.
    Abort(A),
    /// Redraw the screen.
    Redraw,
    /// Quit the picker and select the given item.
    Select,
    /// Restart the picker and receive a new [`Injector`] on the channel.
    Restart(SyncSender<Injector<T, R>>),
}

/// The result of waiting for an update from an [`EventSource`] with a timeout.
///
/// This is quite similar to the standard library
/// [`mpsc::RecvTimeoutError`](std::sync::mpsc::RecvTimeoutError), but also permitting an
/// [`io::Error`] which may result from reading from standard input.
#[non_exhaustive]
pub enum RecvError {
    /// No event was received because we timed out.
    Timeout,
    /// The source is disconnected and there are no more messages.
    Disconnected,
    /// An IO error occurred while trying to read an event.
    IO(io::Error),
}

impl From<io::Error> for RecvError {
    fn from(err: io::Error) -> Self {
        Self::IO(err)
    }
}

impl From<RecvTimeoutError> for RecvError {
    fn from(value: RecvTimeoutError) -> Self {
        match value {
            RecvTimeoutError::Timeout => Self::Timeout,
            RecvTimeoutError::Disconnected => Self::Disconnected,
        }
    }
}

/// An abstraction over sources of [`Event`]s which drive a [`Picker`](crate::Picker).
///
/// Usually, you do not need to implement this trait yourself and can instead use one of the
/// provided implementations:
///
/// - An implementation for [`StdinReader`], which reads key events interactively from standard
///   input and supports custom key bindings.
/// - An implementation for the [`Receiver`] end of a [`sync::mpsc`](std::sync::mpsc) channel.
///
/// The [`Receiver`] implementation means, in most cases, you can simply run an event driver in a
/// separate thread and pass the receiver to the [`Picker`](crate::Picker). This might also be
/// useful when co-existing with other parts of the application which might themselves generate
/// events which are relevant for a picker. Also see the [`StdinEventSender`] struct.
///
/// ## Debouncing
/// The picker automatically debounces incoming events, so you do not need to handle this yourself.
/// However, since there are limitations to the commutativity of events, if the event stream is
/// very overactive, the picker may still lag.
///
/// ## Associated `AbortErr` type
/// The associated `AbortErr` type defines the application-specific error type which may be
/// propagated directly to the picker. This is the same type as present in
/// [`PickError::Aborted`](crate::error::PickError) as well as [`Event::Abort`].
///
/// If you do not need to construct this variant at all, you should set `AbortErr = !` so that
/// you do not need to match on the corresponding [`PickError`](crate::error::PickError) variant.
///
/// The provided implementations for [`StdinReader`] and [`Receiver`] are both generic over a type
/// parameter `A` which defaults to `A = !`. This type parameter is used as `AbortErr` in the
/// provided [`EventSource`] implementation.
///
/// ## Implementation example
/// Here is an example implementation for a `crossbeam::channel::Receiver`. This is identical to
/// the implementation for [`mpsc::Receiver`](std::sync::mpsc::Receiver).
/// ```
/// use std::time::Duration;
///
/// use crossbeam::channel::{Receiver, RecvTimeoutError};
/// use nucleo_picker::event::{Event, EventSource, RecvError};
///
/// struct EventReceiver<T, R, A> {
///     inner: Receiver<Event<T, R, A>>
/// }
///
/// impl<T, R, A> EventSource<T, R> for EventReceiver<T, R, A> {
///     type AbortErr = A;
///
///     fn recv_timeout(&self, duration: Duration) -> Result<Event<T, R, A>, RecvError> {
///         self.inner.recv_timeout(duration).map_err(|err| match err {
///             RecvTimeoutError::Timeout => RecvError::Timeout,
///             RecvTimeoutError::Disconnected => RecvError::Disconnected,
///         })
///     }
/// }
/// ```
///
/// ## Usage example
/// This is a partial usage example illustrating how to use a [`Receiver`]
///
/// In order to complete this example, one should also call
/// [`Picker::pick_with_io`](crate::Picker::pick_with_io) using the
/// receiver end of the channel.
///
/// For the full version of this example with these additional components, visit the [example on
/// GitHub](https://github.com/autobib/nucleo-picker/blob/master/examples/fzf_err_handling.rs)
/// ```
/// use std::{
///     io::{self, BufRead},
///     sync::mpsc::channel,
///     thread::spawn,
/// };
///
/// use nucleo_picker::{
///     event::{Event, StdinEventSender},
///     render::StrRenderer,
///     Picker,
/// };
///
///
/// // initialize a mpsc channel; we use see the 'sender' end to communicate with the picker
/// let (sender, receiver) = channel();
///
/// let mut picker = Picker::new(StrRenderer);
///
/// // spawn a stdin watcher to read keyboard events and send them to the channel
/// // type annotations are only needed here since we do not run the picker with the 'receiver' end,
/// // in the example, which would be enough to determine all of the parameters
/// let stdin_watcher: StdinEventSender<String, StrRenderer, io::Error> =
///     StdinEventSender::with_default_keybindings(sender.clone());
/// spawn(move || match stdin_watcher.watch() {
///     Ok(()) => {
///         // this path occurs when the picker quits and the receiver is dropped so there
///         // is no more work to be done
///     }
///     Err(io_err) => {
///         // we received an IO error while trying to read keyboard events, so we recover the
///         // inner channel and send an `Abort` event to tell the picker to quit immediately
///         //
///         // if we do not send the `Abort` event, or any other event which causes the picker to
///         // quit (such as a `Quit` event), the picker will hang until the thread reading from
///         // standard input completes, which could be a very long time
///         let inner = stdin_watcher.into_sender();
///         // if this fails, the picker already quit
///         let _ = inner.send(Event::Abort(io_err));
///         return;
///     }
/// });
///
/// // read input from standard input
/// let injector = picker.injector();
/// spawn(move || {
///     // in practice, one should also check that `stdin` is not interactive using `IsTerminal`.
///     let stdin = io::stdin();
///     for line in stdin.lines() {
///         match line {
///             Ok(s) => injector.push(s),
///             Err(io_err) => {
///                 // if we encounter an IO error, we send the corresponding error
///                 // to the picker so that it can abort and propagate the error
///                 //
///                 // here, it is also safe to simply ignore the IO error since the picker will
///                 // remain interactive with the items it has already received.
///                 let _ = sender.send(Event::Abort(io_err));
///                 return;
///             }
///         }
///     }
/// });
/// ```
pub trait EventSource<T, R> {
    /// The application-defined abort error propagated to the picker.
    type AbortErr;

    /// Receive a new event, timing out after the provided duration.
    ///
    /// If the receiver times out, the implementation should return a [`RecvError::Timeout`].
    /// If the receiver cannot receive any more events, the implementation should return a
    /// [`RecvError::Disconnected`]. Otherwise, return one of the other variants.
    fn recv_timeout(&self, duration: Duration) -> Result<Event<T, R, Self::AbortErr>, RecvError>;
}

impl<T, R, A> EventSource<T, R> for Receiver<Event<T, R, A>> {
    type AbortErr = A;

    fn recv_timeout(&self, duration: Duration) -> Result<Event<T, R, A>, RecvError> {
        self.recv_timeout(duration).map_err(From::from)
    }
}

/// An [`EventSource`] implementation which reads events from [`io::Stdin`] and maps key
/// events to events using an internal keybinding. The default implementation uses the
/// [`keybind_default`] function for keybindings.
///
/// ## Customizing keybindings
///
/// The default keybindings are documented
/// [here](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md#keyboard-shortcuts). When
/// modifying keybindings, if you are targeting Windows as a platform, you probably want to check
/// for [`KeyEventKind::Press`](crossterm::event::KeyEventKind::Press) or you may get duplicated
/// events.
///
/// ## Example
///
/// Use the [`keybind_default`] function to simplify your implementation of keybindings:
/// ```
/// use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
/// use nucleo_picker::event::{keybind_default, Event, StdinReader};
///
/// /// Keybindings which use the default keybindings, but instead of interrupting on `ctrl + c`,
/// /// instead performs a normal quit action. Generic over all possible `Event` type parameters
/// /// for flexibility.
/// fn keybind_no_interrupt<T, R, A>(key_event: KeyEvent) -> Option<Event<T, R, A>> {
///     match key_event {
///         KeyEvent {
///             kind: KeyEventKind::Press,
///             modifiers: KeyModifiers::CONTROL,
///             code: KeyCode::Char('c'),
///             ..
///         } => Some(Event::Quit),
///         e => keybind_default(e),
///     }
/// }
/// ```
pub struct StdinReader<T, R, A = Infallible, F = fn(KeyEvent) -> Option<Event<T, R, A>>> {
    keybind: F,
    _item: PhantomData<T>,
    _render: PhantomData<R>,
    _abort: PhantomData<A>,
}

impl<T, R, A> Default for StdinReader<T, R, A> {
    fn default() -> Self {
        Self::new(keybind_default)
    }
}

impl<T, R, A, F: Fn(KeyEvent) -> Option<Event<T, R, A>>> StdinReader<T, R, A, F> {
    /// Create a new [`StdinReader`] with keybindings provided by the given closure.
    pub fn new(keybind: F) -> Self {
        Self {
            keybind,
            _item: PhantomData,
            _render: PhantomData,
            _abort: PhantomData,
        }
    }
}

impl<T, R, A, F: Fn(KeyEvent) -> Option<Event<T, R, A>>> EventSource<T, R>
    for StdinReader<T, R, A, F>
{
    type AbortErr = A;

    fn recv_timeout(&self, duration: Duration) -> Result<Event<T, R, A>, RecvError> {
        if poll(duration)? {
            if let Some(event) = convert_crossterm_event(read()?, &self.keybind) {
                return Ok(event);
            }
        };
        Err(RecvError::Timeout)
    }
}

/// A wrapper for a [`Sender`] which reads events from standard input and sends them to the
/// channel.
///
/// The internal implementation is identical to the [`StdinReader`] struct, but instead of
/// generating the events directly, sends them to the channel.
pub struct StdinEventSender<T, R, A = Infallible, F = fn(KeyEvent) -> Option<Event<T, R, A>>> {
    sender: Sender<Event<T, R, A>>,
    keybind: F,
}

impl<T, R, A> StdinEventSender<T, R, A> {
    /// Initialize a new [`StdinEventSender`] with default keybindings in the provided channel.
    pub fn with_default_keybindings(sender: Sender<Event<T, R, A>>) -> Self {
        Self {
            sender,
            keybind: keybind_default,
        }
    }
}

impl<T, R, A, F: Fn(KeyEvent) -> Option<Event<T, R, A>>> StdinEventSender<T, R, A, F> {
    /// Initialize a new [`StdinEventSender`] with the given keybindings in the provided channel.
    pub fn new(sender: Sender<Event<T, R, A>>, keybind: F) -> Self {
        Self { sender, keybind }
    }

    /// Convert into the inner [`Sender<Event>`] to send further events when finished.
    pub fn into_sender(self) -> Sender<Event<T, R, A>> {
        self.sender
    }

    /// Watch for events until either the receiver is dropped (in which case `Ok(())` is returned),
    /// or there is an IO error.
    pub fn watch(&self) -> io::Result<()> {
        loop {
            if let Some(event) = convert_crossterm_event(read()?, &self.keybind) {
                if self.sender.send(event).is_err() {
                    return Ok(());
                }
            }
        }
    }
}
