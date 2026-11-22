use std::{
    io::{self, BufWriter, IsTerminal},
    marker::PhantomData,
    sync::mpsc::Sender,
    time::Duration,
};

use crossterm::event::{Event as CrosstermEvent, KeyEvent, poll, read};

use crate::{
    Injector, Render,
    error::PickError,
    event::{
        Event, EventSource, KeybindDefault, PickerStatus, PromptEvent, RecvError, keybind_default,
    },
    interface::Interface,
    observer::Notifier,
    run::{self, RunnerConfig},
    session::Model,
    terminal::CrosstermTerminal,
};

/// Convert a crossterm event into an [`Event`], mapping key events with the given keybindings.
fn convert_crossterm_event<A, F: FnMut(KeyEvent) -> Option<Event<A>>>(
    ct_event: CrosstermEvent,
    mut keybind: F,
) -> Option<Event<A>> {
    match ct_event {
        CrosstermEvent::Key(key_event) => keybind(key_event),
        CrosstermEvent::Resize(_, _) => Some(Event::Redraw),
        CrosstermEvent::Paste(contents) => Some(Event::Prompt(PromptEvent::Paste(contents))),
        _ => None,
    }
}

/// Run an interface interactively with its default keybindings.
///
/// Uses the terminal and error behavior described by [`Picker::pick`](crate::Picker::pick).
#[inline]
pub(crate) fn pick<'items, T, R, I>(
    model: &'items mut Model<T, R>,
    interface: &mut I,
    config: RunnerConfig,
    restart_notifier: Option<&Notifier<Injector<T, R>>>,
    status_notifier: Option<&Notifier<PickerStatus>>,
) -> Result<I::Output<'items>, PickError>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T> + KeybindDefault,
{
    pick_with_keybind(
        model,
        interface,
        config,
        restart_notifier,
        status_notifier,
        I::keybind_default,
    )
}

/// Run an interface interactively with custom keybindings.
///
/// Uses the terminal and error behavior described by
/// [`Picker::pick_with_keybind`](crate::Picker::pick_with_keybind).
#[inline]
pub(crate) fn pick_with_keybind<'items, T, R, I, F>(
    model: &'items mut Model<T, R>,
    interface: &mut I,
    config: RunnerConfig,
    restart_notifier: Option<&Notifier<Injector<T, R>>>,
    status_notifier: Option<&Notifier<PickerStatus>>,
    keybind: F,
) -> Result<I::Output<'items>, PickError>
where
    T: Send + Sync + 'static,
    R: Render<T>,
    I: Interface<T>,
    F: FnMut(KeyEvent) -> Option<Event>,
{
    let stderr = io::stderr().lock();
    if stderr.is_terminal() {
        run::pick_impl(
            model,
            interface,
            config,
            restart_notifier,
            status_notifier,
            StdinReader::new(keybind),
            &mut CrosstermTerminal::new(&mut BufWriter::new(stderr)),
        )
    } else {
        Err(PickError::NotInteractive)
    }
}

/// An [`EventSource`] implementation which reads events from the terminal and maps key
/// events to events using a keybinding closure.
///
/// The default implementation uses the [`keybind_default`] function for keybindings.
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
/// Use [`keybind_single`](crate::event::keybind_single) to simplify your implementation of keybindings:
///
/// ```
/// use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
/// use nucleo_picker::event::{keybind_single, Event, StdinReader};
///
/// /// Keybindings which use single-selection keybindings, but instead of interrupting on `ctrl + c`,
/// /// performs a normal quit action. Generic over all possible `Event` type parameters
/// /// for flexibility.
/// fn keybind_no_interrupt<A>(key_event: KeyEvent) -> Option<Event<A>> {
///     match key_event {
///         KeyEvent {
///             kind: KeyEventKind::Press,
///             modifiers: KeyModifiers::CONTROL,
///             code: KeyCode::Char('c'),
///             ..
///         } => Some(Event::Quit),
///         e => keybind_single(e),
///     }
/// }
/// ```
pub struct StdinReader<A = !, F = fn(KeyEvent) -> Option<Event<A>>> {
    keybind: F,
    _abort: PhantomData<A>,
}

impl<A> Default for StdinReader<A> {
    fn default() -> Self {
        Self::new(keybind_default)
    }
}

impl<A, F: FnMut(KeyEvent) -> Option<Event<A>>> StdinReader<A, F> {
    /// Create a new [`StdinReader`] with keybindings provided by the given closure.
    pub fn new(keybind: F) -> Self {
        Self {
            keybind,
            _abort: PhantomData,
        }
    }
}

impl<A, F: FnMut(KeyEvent) -> Option<Event<A>>> EventSource for StdinReader<A, F> {
    type AbortErr = A;

    fn recv_timeout(&mut self, duration: Duration) -> Result<Event<A>, RecvError> {
        if poll(duration)?
            && let Some(event) = convert_crossterm_event(read()?, &mut self.keybind)
        {
            return Ok(event);
        }
        Err(RecvError::Timeout)
    }
}

/// A wrapper for a [`Sender`] which reads events from the terminal and sends them to the
/// channel.
///
/// The internal implementation is identical to the [`StdinReader`] struct, but instead of
/// generating the events directly, sends them to the channel.
pub struct StdinEventSender<A = !, F = fn(KeyEvent) -> Option<Event<A>>> {
    sender: Sender<Event<A>>,
    keybind: F,
}

impl<A> StdinEventSender<A> {
    /// Initialize a new [`StdinEventSender`] with default keybindings in the provided channel.
    #[must_use]
    pub fn with_default_keybindings(sender: Sender<Event<A>>) -> Self {
        Self {
            sender,
            keybind: keybind_default,
        }
    }
}

impl<A, F: Fn(KeyEvent) -> Option<Event<A>>> StdinEventSender<A, F> {
    /// Watch for terminal IO events until either the receiver is dropped (in which case `Ok(())` is returned),
    /// or there is an IO error while reading from standard input. This method will block the
    /// current thread until the channel disconnects or a read fails.
    ///
    /// This method is only compatible with keybindings which do not mutate internal state. For a
    /// version which permits mutation, see [`watch_mut`](Self::watch_mut).
    pub fn watch(&self) -> io::Result<()> {
        loop {
            if let Some(event) = convert_crossterm_event(read()?, &self.keybind)
                && self.sender.send(event).is_err()
            {
                return Ok(());
            }
        }
    }
}

impl<A, F: FnMut(KeyEvent) -> Option<Event<A>>> StdinEventSender<A, F> {
    /// Initialize a new [`StdinEventSender`] with the given keybindings in the provided channel.
    pub fn new(sender: Sender<Event<A>>, keybind: F) -> Self {
        Self { sender, keybind }
    }

    /// Convert into the inner [`Sender<Event>`] to send further events when finished.
    pub fn into_sender(self) -> Sender<Event<A>> {
        self.sender
    }

    /// Watch for events until either the receiver is dropped (in which case `Ok(())` is returned),
    /// or there is an IO error while reading from standard input. This method will block the
    /// current thread until the channel disconnects or a read fails.
    ///
    /// If the mutable self reference is inconvenient and your keybindings do not mutate internal
    /// state, use [`watch`](Self::watch).
    pub fn watch_mut(&mut self) -> io::Result<()> {
        loop {
            if let Some(event) = convert_crossterm_event(read()?, &mut self.keybind)
                && self.sender.send(event).is_err()
            {
                return Ok(());
            }
        }
    }
}
