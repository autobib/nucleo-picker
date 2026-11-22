use super::*;
use crate::preview::Pane;

#[test]
fn keybindings_preserve_base_overrides_and_add_preview_fallbacks() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use nucleo_picker_core::event::MatchListEvent;

    struct Base;
    impl KeybindDefault for Base {
        fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
            if key == KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL) {
                Some(Event::Quit)
            } else {
                <Multi as KeybindDefault>::keybind_default(key)
            }
        }
    }

    type Adapter = UncachedPreview<Base, fn(&&str, &mut Pane)>;
    assert!(matches!(
        Adapter::keybind_default::<()>(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL)),
        Some(Event::Quit)
    ));
    assert!(matches!(
        Adapter::keybind_default::<()>(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT)),
        Some(Event::Preview(PreviewEvent::Down(1)))
    ));
    assert!(matches!(
        Adapter::keybind_default::<()>(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
        Some(Event::MatchList(MatchListEvent::ToggleDown(1)))
    ));
}
