use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[cfg(feature = "preview-cache")]
use crate::interface::CachedPreview;
use crate::interface::{Multi, Single, UncachedPreview};

use super::{Event, LayoutEvent, MatchListEvent, PreviewEvent, PromptEvent};

#[cfg(test)]
mod tests;

/// Interfaces which know how to automatically map terminal input to events.
///
/// This trait is used by [`Picker::pick`](crate::Picker::pick) to decide which keybindings to use
/// when mapping events from the terminal. The built-in interfaces all provide default keybindings.
pub trait KeybindDefault {
    /// The default keybindings for an interface.
    ///
    /// This method is generic over the abort type `A` for compatibility with arbitrary application
    /// errors. Interfaces use this to specialize event handling to only emit events which this
    /// specific interface requires. See the
    /// [keybind specialization documentation](crate::event::keybind_default#specialized-bindings)
    /// for more details.
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_default(key)
    }
}

impl<I: KeybindDefault + ?Sized> KeybindDefault for &mut I {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key)
    }
}

impl KeybindDefault for Single {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_single(key)
    }
}

impl KeybindDefault for Multi {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        keybind_multi(key)
    }
}

impl<I: KeybindDefault, F> KeybindDefault for UncachedPreview<I, F> {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key).or_else(|| keybind_passthrough_preview(key).ok())
    }
}

#[cfg(feature = "preview-cache")]
impl<I: KeybindDefault, P> KeybindDefault for CachedPreview<I, P> {
    #[inline]
    fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
        I::keybind_default(key).or_else(|| keybind_passthrough_preview(key).ok())
    }
}

/// The default keybindings.
///
/// This includes all keybindings supported by the enabled features. These are the keybindings
/// used in the [`Default`] implementation for [`StdinReader`](crate::event::StdinReader). A list of the
/// bindings can be found in the [picker usage
/// documentation](https://github.com/autobib/nucleo-picker/blob/master/USAGE.md).
///
/// # Generic parameter
///
/// This function is generic over the type parameter `A`, which is the associated type
/// [`AbortErr`](crate::event::EventSource::AbortErr) of an [`EventSource`](crate::event::EventSource).
/// However, the type parameter does not appear anywhere in the function arguments since an
/// [`Event::Abort`] is never produced by the default keybindings. This type parameter is simply
/// here for flexibility to generate events of a particular type when used in situations where `A`
/// is not the default `!`.
///
/// # Specialized bindings
///
/// If you know that certain events are not required by a given picker, you can use one of the
/// specialized keybind functions.
///
/// Binding | Multiple-selection events? | Preview events?
/// --------|----------------------------|----------------
/// [`keybind_single`] | No | No
/// [`keybind_multi`] | Yes | No
/// [`keybind_single_preview`] | No | Yes
/// [`keybind_multi_preview`] | Yes | Yes
///
/// The default keybindings will work in any selection mode since unused events are
/// ignored by the picker. The purpose of the specialized bindings is to *slightly reduce overhead
/// by not emitting the event at all*.
///
/// In most cases, you should just use the default keybindings since the additional
/// overhead is quite minimal. Using these keybindings will mean that *certain events are never
/// emitted, independently of picker support*. For example, using [`keybind_single`] with a
/// multi-selection picker will prevent the user from making more than 1 selection, since the
/// multi-selection keybinding is ignored entirely.
#[inline]
pub fn keybind_default<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_multi_preview(key_event)
}

/// Keybindings which only emit single-selection non-preview events.
///
/// These are the keybindings for the [`Single`](crate::Single) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[inline]
pub fn keybind_single<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event).ok()
}

/// Keybindings which emit multiple-selection but non-preview events.
///
/// These are the keybindings for the [`Multi`](crate::Multi) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[inline]
pub fn keybind_multi<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event)
        .or_else(keybind_passthrough_multi)
        .ok()
}

/// Keybindings which only emit single-selection events and preview events.
///
/// These are the keybindings for the [`CachedPreview<Single, P>`](crate::CachedPreview) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for more details.
#[inline]
pub fn keybind_single_preview<A>(key_event: KeyEvent) -> Option<Event<A>> {
    keybind_passthrough_core(key_event)
        .or_else(keybind_passthrough_preview)
        .ok()
}

/// Keybindings which only emit multi-selection and preview events.
///
/// These are the keybindings for the [`CachedPreview<Multi, P>`](crate::CachedPreview) interface.
/// See the [caveats on specialized bindings](keybind_default#specialized-bindings) for
/// more details.
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
