use std::cell::RefCell;

use super::*;
use crate::{
    render::StrRenderer,
    session::{Driver, DriverOptions},
};

fn driver() -> Driver<&'static str, StrRenderer> {
    let mut driver = DriverOptions::new().sort_results(false).driver(StrRenderer);
    driver.push_batch(["alpha", "beta", "alpha"]);
    driver.filter().unwrap();
    driver
}

fn context<'a>(
    driver: &'a Driver<&'static str, StrRenderer>,
    rank: Option<u32>,
) -> Context<'a, &'static str> {
    Context::new(driver.matches())
        .with_rank(rank)
        .with_preview_enabled(true)
}

fn pane<'a, I: Interface<&'static str>>(interface: &'a I, state: &'a I::State) -> &'a Pane {
    match interface.preview_content(state) {
        Content::Ready(pane) => pane,
        _ => panic!("expected ready preview"),
    }
}

#[test]
fn identity_changes_and_refresh_regenerate_clean_panes_but_hidden_frames_wait() {
    let driver = driver();
    let generated = RefCell::new(Vec::new());
    let mut interface = UncachedPreview::new_single(|item: &&'static str, pane: &mut Pane| {
        assert_eq!(pane.buffer().lines().len(), 1);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
        assert_eq!(pane.offset(), (0, 0));
        assert!(!pane.line_numbers());
        assert!(!pane.is_err());
        generated.borrow_mut().push(*item);
        pane.push_text(item);
        pane.set_offset(7, 9);
        pane.set_line_numbers(true);
        pane.set_err(true);
    });
    let mut state = interface.init(&context(&driver, None));
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    for (rank, redraw, count) in [
        (0, Components::PREVIEW, 1),
        (0, Components::NONE, 1),
        (2, Components::PREVIEW, 2),
        (0, Components::PREVIEW, 3),
    ] {
        assert_eq!(
            interface.before_frame(
                &mut state,
                &context(&driver, Some(rank)),
                true,
                Instant::now()
            ),
            redraw
        );
        assert_eq!(generated.borrow().len(), count);
        let pane = pane(&interface, &state);
        assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
        assert_eq!(pane.offset(), (7, 9));
        assert!(pane.line_numbers());
        assert!(pane.is_err());
    }

    let cx = context(&driver, Some(0));
    for _ in 0..2 {
        let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &cx);
    }
    let hidden = context(&driver, Some(1)).with_preview_enabled(false);
    assert!(
        interface
            .before_frame(&mut state, &hidden, true, Instant::now())
            .is_none()
    );
    assert_eq!(pane(&interface, &state).offset(), (7, 9));
    assert_eq!(generated.borrow().len(), 3);
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, Instant::now()),
        Components::PREVIEW
    );
    assert_eq!(generated.borrow().len(), 4);
    let empty = context(&driver, None);
    assert_eq!(
        interface.before_frame(&mut state, &empty, true, Instant::now()),
        Components::PREVIEW
    );
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    assert!(
        interface
            .before_frame(&mut state, &empty, false, Instant::now())
            .is_none()
    );
}

#[test]
fn events_require_the_displayed_identity_and_use_delivered_dimensions() {
    let driver = driver();
    let mut interface = UncachedPreview::new_single(|_: &&str, pane: &mut Pane| {
        pane.push_text("abcdefghijklmnop\n1\n2\n3\n4\n5\n6\n7\n8\n9");
    });
    let cx = context(&driver, Some(0));
    let other = context(&driver, Some(1));
    let mut state = interface.init(&cx);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 3,
        }),
    );
    let _ = interface.before_frame(&mut state, &cx, true, Instant::now());
    for event in [
        PreviewEvent::Down(1),
        PreviewEvent::ToggleLineNumbers,
        PreviewEvent::Refresh,
    ] {
        assert!(interface.preview_event(&mut state, event, &other).is_none());
    }
    assert_eq!(pane(&interface, &state).offset(), (0, 0));
    assert!(!pane(&interface, &state).line_numbers());
    assert!(
        interface
            .before_frame(&mut state, &cx, false, Instant::now())
            .is_none()
    );
    for event in [
        PreviewEvent::Right(4),
        PreviewEvent::PageDown(1),
        PreviewEvent::ToggleLineNumbers,
    ] {
        assert_eq!(
            interface.preview_event(&mut state, event, &cx),
            Components::PREVIEW
        );
    }
    assert_eq!(pane(&interface, &state).offset(), (4, 3));
    assert!(
        interface
            .preview_event(&mut state, PreviewEvent::SetLineNumbers(true), &cx)
            .is_none()
    );
    let _ = interface.preview_event(&mut state, PreviewEvent::AlignBottom, &cx);
    assert_eq!(pane(&interface, &state).offset(), (4, 7));

    for size in [
        None,
        Some(Dimensions {
            width: 8,
            height: 0,
        }),
    ] {
        interface.preview_resize(&mut state, size);
        assert_eq!(pane(&interface, &state).offset(), (4, 7));
        assert!(
            interface
                .preview_event(&mut state, PreviewEvent::Down(1), &cx)
                .is_none()
        );
    }
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 6,
        }),
    );
    assert_eq!(pane(&interface, &state).offset(), (4, 4));
    let _ = interface.preview_event(&mut state, PreviewEvent::PageUp(1), &cx);
    assert_eq!(pane(&interface, &state).offset(), (4, 0));
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 0,
            height: 6,
        }),
    );
    assert!(
        interface
            .preview_event(&mut state, PreviewEvent::Right(1), &cx)
            .is_none()
    );
}

#[test]
fn navigation_uses_grapheme_widths_across_preview_changes() {
    let driver = driver();
    let mut interface = UncachedPreview::new_single(|_: &&str, pane: &mut Pane| {
        pane.push_text("界界界界");
    });
    let cx = context(&driver, Some(0));
    let mut state = interface.init(&cx);
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 2,
            height: 1,
        }),
    );
    let _ = interface.before_frame(&mut state, &cx, true, Instant::now());
    let _ = interface.preview_event(&mut state, PreviewEvent::AlignRight, &cx);
    assert_eq!(pane(&interface, &state).offset(), (6, 0));

    for rank in [0, 1, 0] {
        let cx = context(&driver, Some(rank));
        let _ = interface.before_frame(&mut state, &cx, true, Instant::now());
        let _ = interface.preview_event(&mut state, PreviewEvent::AlignRight, &cx);
        assert_eq!(pane(&interface, &state).offset(), (6, 0));
        let _ = interface.preview_event(&mut state, PreviewEvent::AlignLeft, &cx);
        let _ = interface.preview_event(&mut state, PreviewEvent::Right(usize::MAX), &cx);
        assert_eq!(pane(&interface, &state).offset(), (6, 0));
        assert!(
            interface
                .preview_event(&mut state, PreviewEvent::Right(1), &cx)
                .is_none()
        );
    }
    interface.on_restart(&mut state, &cx);
    let _ = interface.before_frame(&mut state, &cx, true, Instant::now());
    let _ = interface.preview_event(&mut state, PreviewEvent::AlignRight, &cx);
    assert_eq!(pane(&interface, &state).offset(), (6, 0));
}

#[test]
fn restart_invalidates_content_even_when_item_values_and_indices_are_reused() {
    let mut driver = driver();
    let generated = RefCell::new(Vec::new());
    let mut interface = UncachedPreview::new_single(|item: &&'static str, pane: &mut Pane| {
        generated.borrow_mut().push(*item);
        pane.push_text(item);
    });
    let cx = context(&driver, Some(0));
    let old_id = cx.hovered().unwrap().id();
    let mut state = interface.init(&cx);
    let _ = interface.before_frame(&mut state, &cx, true, Instant::now());
    interface.on_restart(&mut state, &cx);
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    driver.restart();
    driver.push_batch(["alpha"]);
    driver.filter().unwrap();
    let cx = context(&driver, Some(0));
    assert_ne!(cx.hovered().unwrap().id(), old_id);
    assert_eq!(
        interface.before_frame(&mut state, &cx, true, Instant::now()),
        Components::PREVIEW
    );
    assert_eq!(*generated.borrow(), ["alpha", "alpha"]);
}

#[test]
fn keybindings_preserve_base_overrides_and_add_preview_fallbacks() {
    use crate::event::MatchListEvent;
    use crossterm::event::{KeyCode, KeyModifiers};

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
