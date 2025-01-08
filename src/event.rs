//! # Event handling
//!
//! This module defines the core [`Event`] type handled by a [`Picker`](crate::Picker), which
//! defines an interactive update to the picker state.
//!
//! By default, the [`Picker::pick`](crate::Picker::pick) reads events from the terminal and maps
//! those events to [`Event`]s. The process of reading events is encapsulated in the
//! [`EventSource`] trait, which you can implement yourself and pass directly to the picker using
//! the [`Picker::pick_with_io`](crate::Picker::pick_with_io).
//!
//! Jump to:
//! - The [`EventSource`] trait.
//! - The [`StdinReader`], for automatically reading events from standard input, with customizable
//!   keybindings.
//! - The [default keybindings](keybind_default)

mod bind;

use std::{
    io,
    sync::mpsc::{Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

use self::bind::convert_crossterm_event;
use crossterm::event::{poll, read, KeyEvent};

pub use self::bind::keybind_default;
pub use crate::match_list::MatchListEvent;
pub use crate::prompt::PromptEvent;

/// An event which controls the picker behaviour.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// Modify the prompt.
    Prompt(PromptEvent),
    /// Modify the list of matches.
    MatchList(MatchListEvent),
    /// Quit the picker (no selection).
    Quit,
    /// Quit the picker (no selection) if the prompt is empty.
    QuitPromptEmpty,
    /// Abort the picker (error).
    Abort,
    /// Resize the screen.
    Redraw,
    /// Quit the picker and select the given item.
    Select,
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
/// built-in implementations:
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
/// Note however that the debouncing is fundamentally limited because of the limitations to
/// commutativity of events. If the event stream is overactive, the picker may lag.
///
/// ## Example
/// Here is an example implementation for a `crossbeam::channel::Receiver`. This is identical to
/// the implementation for [`mpsc::Receiver`](std::sync::mpsc::Receiver).
/// ```
/// use std::time::Duration;
///
/// use crossbeam::channel::{Receiver, RecvTimeoutError};
/// use nucleo_picker::event::{Event, EventSource, RecvError};
///
/// struct ReceiverWrapper {
///     inner: Receiver<Event>
/// }
///
/// impl EventSource for ReceiverWrapper {
///     fn recv_timeout(&self, duration: Duration) -> Result<Event, RecvError> {
///         self.inner.recv_timeout(duration).map_err(|err| match err {
///             RecvTimeoutError::Timeout => RecvError::Timeout,
///             RecvTimeoutError::Disconnected => RecvError::Disconnected,
///         })
///     }
/// }
/// ```
pub trait EventSource {
    /// Receive a new event, timing out after the provided duration.
    ///
    /// If the receive times out, the implementation should return a [`RecvError::Timeout`].
    /// If the receiver cannot receive any more events, the implementation should return a
    /// [`RecvError::Disconnected`]. Otherwise, return one of the other variants.
    fn recv_timeout(&self, duration: Duration) -> Result<Event, RecvError>;
}

impl EventSource for Receiver<Event> {
    fn recv_timeout(&self, duration: Duration) -> Result<Event, RecvError> {
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
/// /// Keybindings which use the default keybindings, but instead of aborting on `ctrl + c`,
/// /// simply perform a normal quit action.
/// fn keybind_no_abort(key_event: KeyEvent) -> Option<Event> {
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
///
/// let no_abort_reader = StdinReader::new(keybind_no_abort);
/// ```
pub struct StdinReader<F = fn(KeyEvent) -> Option<Event>> {
    keybind: F,
}

impl Default for StdinReader {
    fn default() -> Self {
        Self {
            keybind: keybind_default,
        }
    }
}

impl<F: Fn(KeyEvent) -> Option<Event>> StdinReader<F> {
    /// Create a new [`StdinReader`] with keybindings provided by the given closure.
    pub fn new(keybind: F) -> Self {
        Self { keybind }
    }
}

impl<F: Fn(KeyEvent) -> Option<Event>> EventSource for StdinReader<F> {
    fn recv_timeout(&self, duration: Duration) -> Result<Event, RecvError> {
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
pub struct StdinEventSender<F = fn(KeyEvent) -> Option<Event>> {
    sender: Sender<Event>,
    keybind: F,
}

impl StdinEventSender {
    /// Initialize a new [`StdinEventSender`] with default keybindings in the provided channel.
    pub fn with_default_keybindings(sender: Sender<Event>) -> Self {
        Self {
            sender,
            keybind: keybind_default,
        }
    }
}

impl<F: Fn(KeyEvent) -> Option<Event>> StdinEventSender<F> {
    /// Initialize a new [`StdinEventSender`] with the given keybindings in the provided channel.
    pub fn new(sender: Sender<Event>, keybind: F) -> Self {
        Self { sender, keybind }
    }

    /// Watch for events until either the receiver is dropped (in which case `Ok(())` is returned),
    /// or there is an IO error.
    pub fn watch(&self) -> Result<(), io::Error> {
        loop {
            if let Some(event) = convert_crossterm_event(read()?, &self.keybind) {
                if self.sender.send(event).is_err() {
                    return Ok(());
                }
            }
        }
    }
}
