use crossterm::event::{Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{Event, MatchListEvent, PromptEvent};
#[cfg(feature = "preview")]
use super::{LayoutEvent, PreviewEvent};

/// The default keybindings.
///
/// This includes all keybindings supported by the enabled features. These are the keybindings
/// used in the [`Default`] implementation for [`StdinReader`](super::StdinReader).
///
/// # Generic parameter
///
/// This function is generic over the type parameter `A`, which is the associated type
/// [`AbortErr`](super::EventSource::AbortErr) of an [`EventSource`](super::EventSource).
/// However, the type parameter does not appear anywhere in the function arguments since an
/// [`Event::Abort`] is never produced by the default keybindings. This type parameter is simply
/// here for flexibility to generate events of a particular type when used in situations where `A`
/// is not the default `!`.
///
/// # Preview bindings
///
/// When the `preview` feature is enabled, the default keybindings include keybindings which target
/// the preview pane.
///
/// # Specialized bindings
///
/// If you know certain events are not required by a given picker, you can use one of the reduced
/// keybinding sets.
///
/// Binding | Multiple-selection events? | Preview events?
/// --------|----------------------------|----------------
/// [`keybind_single`] | No | No
/// [`keybind_multi`] | Yes | No
/// [`keybind_single_preview`] | No | Yes
/// [`keybind_multi_preview`] | Yes | Yes
///
/// The default keybindings will work in any selection mode, since the unused events will simply be
/// ignored by the picker. The purpose of the specialized bindings is to *slightly reduce overhead
/// by not emitting the event at all*.
///
/// In most cases, you should just use the default keybindings since the additional
/// overhead is quite minimal. Using these keybindings will mean that *certain events are never
/// emitted, independently of picker support*. For example, using [`keybind_single`] with a
/// multi-selection picker will prevent the user from making more than 1 selection, regardless of
/// support in the picker itself.
#[inline]
pub fn keybind_default<A>(key_event: KeyEvent) -> Option<Event<A>> {
    #[cfg(feature = "preview")]
    {
        keybind_multi_preview(key_event)
    }
    #[cfg(not(feature = "preview"))]
    {
        keybind_multi(key_event)
    }
}

/// Keybindings which do not emit [multi-selection events](MatchListEvent#multi-selection-events).
///
/// This wrapper type has been deprecated in favour of [`keybind_single`] and [`keybind_single_preview`].
#[deprecated(
    since = "0.13.0",
    note = "Use `keybind_single` or `keybind_single_preview` instead"
)]
#[inline]
pub fn keybind_no_multi<A>(key_event: KeyEvent) -> Option<Event<A>> {
    #[cfg(feature = "preview")]
    {
        keybind_single_preview(key_event)
    }
    #[cfg(not(feature = "preview"))]
    {
        keybind_single(key_event)
    }
}

/// Keybindings which only emit single-selection non-preview events.
///
/// These are the keybindings for the [`Single`](crate::session::Single) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[inline]
pub fn keybind_single<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event).ok()
}

/// Keybindings which emit multiple-selection but non-preview events.
///
/// These are the keybindings for the [`Multi`](crate::session::Multi) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[inline]
pub fn keybind_multi<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event)
        .or_else(keybind_passthrough_multi)
        .ok()
}

/// Keybindings which only emit single-selection events and preview events.
///
/// These are the keybindings for the [`WithPreview<Single, P>`](crate::session::WithPreview) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
#[inline]
pub fn keybind_single_preview<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event)
        .or_else(keybind_passthrough_preview)
        .ok()
}

/// Keybindings which only emit multi-selection and preview events.
///
/// These are the keybindings for the [`WithPreview<Multi, P>`](crate::session::WithPreview) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for
/// more details.
#[cfg(feature = "preview")]
#[cfg_attr(docsrs, doc(cfg(feature = "preview")))]
#[inline]
pub fn keybind_multi_preview<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event)
        .or_else(keybind_passthrough_multi)
        .or_else(keybind_passthrough_preview)
        .ok()
}

/// A composable set of keybindings for multi-selection mode.
#[inline]
fn keybind_passthrough_multi<A>(key_event: KeyEvent) -> Result<Event<A>, KeyEvent> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Tab => Ok(Event::MatchList(MatchListEvent::ToggleDown(1))),
            KeyCode::BackTab => Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        } => match code {
            KeyCode::Char('-') => Ok(Event::MatchList(MatchListEvent::UnqueueAll)),
            KeyCode::Char('=') => Ok(Event::MatchList(MatchListEvent::QueueMatches)),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code: KeyCode::BackTab,
            ..
        } => Ok(Event::MatchList(MatchListEvent::ToggleUp(1))),
        e => Err(e),
    }
}

/// A composable set of keybindings for preview-pane manipulation.
#[cfg(feature = "preview")]
#[inline]
fn keybind_passthrough_preview<A>(key_event: KeyEvent) -> Result<Event<A>, KeyEvent> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code: KeyCode::Char('r'),
            ..
        } => Ok(Event::Preview(PreviewEvent::Refresh)),
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::ALT,
            code,
            ..
        } => match code {
            KeyCode::Char('n') => Ok(Event::Preview(PreviewEvent::ToggleLineNumbers)),
            KeyCode::Char('p') => Ok(Event::Layout(LayoutEvent::TogglePreview)),
            KeyCode::Char(',') => Ok(Event::Layout(LayoutEvent::MoveDividerLeft(1))),
            KeyCode::Char('.') => Ok(Event::Layout(LayoutEvent::MoveDividerRight(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code,
            ..
        } => match code {
            KeyCode::Up => Ok(Event::Preview(PreviewEvent::Up(1))),
            KeyCode::Down => Ok(Event::Preview(PreviewEvent::Down(1))),
            KeyCode::Left => Ok(Event::Preview(PreviewEvent::Left(1))),
            KeyCode::Right => Ok(Event::Preview(PreviewEvent::Right(1))),
            KeyCode::Home => Ok(Event::Preview(PreviewEvent::AlignLeft)),
            KeyCode::End => Ok(Event::Preview(PreviewEvent::AlignRight)),
            KeyCode::PageUp => Ok(Event::Preview(PreviewEvent::PageUp(1))),
            KeyCode::PageDown => Ok(Event::Preview(PreviewEvent::PageDown(1))),
            _ => Err(key_event),
        },
        _ => Err(key_event),
    }
}

/// The core keybindings, in a composable format.
#[inline]
fn keybind_passthrough_core<A>(key_event: KeyEvent) -> Result<Event<A>, KeyEvent> {
    match key_event {
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::NONE,
            code,
            ..
        } => match code {
            KeyCode::Esc => Ok(Event::Quit),
            KeyCode::Up => Ok(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Down => Ok(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Left => Ok(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Right => Ok(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Home => Ok(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::End => Ok(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char(ch) => Ok(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::Backspace => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Ok(Event::Select),
            KeyCode::Delete => Ok(Event::Prompt(PromptEvent::Delete(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::CONTROL,
            code,
            ..
        } => match code {
            KeyCode::Char('c') => Ok(Event::UserInterrupt),
            KeyCode::Char('d') => Ok(Event::QuitPromptEmpty),
            KeyCode::Char('0') => Ok(Event::MatchList(MatchListEvent::Reset)),
            KeyCode::Char('g' | 'q') => Ok(Event::Quit),
            KeyCode::Char('k' | 'p') => Ok(Event::MatchList(MatchListEvent::Up(1))),
            KeyCode::Char('j' | 'n') => Ok(Event::MatchList(MatchListEvent::Down(1))),
            KeyCode::Char('b') => Ok(Event::Prompt(PromptEvent::Left(1))),
            KeyCode::Char('f') => Ok(Event::Prompt(PromptEvent::Right(1))),
            KeyCode::Char('a') => Ok(Event::Prompt(PromptEvent::ToStart)),
            KeyCode::Char('e') => Ok(Event::Prompt(PromptEvent::ToEnd)),
            KeyCode::Char('h') => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Char('w') => Ok(Event::Prompt(PromptEvent::BackspaceWord(1))),
            KeyCode::Char('u') => Ok(Event::Prompt(PromptEvent::ClearBefore)),
            KeyCode::Char('o') => Ok(Event::Prompt(PromptEvent::ClearAfter)),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::ALT,
            code,
            ..
        } => match code {
            KeyCode::Char('f') => Ok(Event::Prompt(PromptEvent::WordRight(1))),
            KeyCode::Char('b') => Ok(Event::Prompt(PromptEvent::WordLeft(1))),
            _ => Err(key_event),
        },
        KeyEvent {
            kind: KeyEventKind::Press,
            modifiers: KeyModifiers::SHIFT,
            code,
            ..
        } => match code {
            KeyCode::Char(ch) => Ok(Event::Prompt(PromptEvent::Insert(ch))),
            KeyCode::Backspace => Ok(Event::Prompt(PromptEvent::Backspace(1))),
            KeyCode::Enter => Ok(Event::MatchList(MatchListEvent::Down(1))),
            _ => Err(key_event),
        },
        _ => Err(key_event),
    }
}

/// Convert a crossterm event into an [`Event`], mapping key events with the given keybindings.
pub fn convert_crossterm_event<A, F: FnMut(KeyEvent) -> Option<Event<A>>>(
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
