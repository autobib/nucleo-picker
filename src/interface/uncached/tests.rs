use std::cell::{Cell, RefCell};

use super::*;
use crate::{Picker, PickerOptions, render::StrRenderer};

fn picker() -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new().sort_results(false).picker(StrRenderer);
    picker.push_batch(["alpha", "beta", "alpha"]);
    picker.filter().unwrap();
    picker
}

fn context<'a>(
    picker: &'a Picker<&'static str, StrRenderer>,
    rank: Option<u32>,
) -> Context<'a, &'static str> {
    Context::new(&picker.core.engine)
        .with_rank(rank)
        .with_preview_enabled(true)
}

fn pane<'a, I: Interface<&'static str>>(interface: &'a I, state: &'a I::State) -> &'a Pane {
    match interface.preview_content(state) {
        Content::Ready(pane) => pane,
        _ => panic!("expected a ready pane"),
    }
}

#[test]
fn generation_tracks_identity_and_refresh_and_preserves_presentation() {
    let picker = picker();
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
    let mut state = interface.init(&context(&picker, None));
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    assert!(generated.borrow().is_empty());
    let deadline = Instant::now();

    for (rank, changed, redraw, count) in [
        (Some(0), true, Redraw::PREVIEW, 1),
        (Some(0), false, Redraw::NONE, 1),
        (Some(0), true, Redraw::NONE, 1),
        (Some(1), false, Redraw::PREVIEW, 2),
        (Some(0), false, Redraw::PREVIEW, 3),
        (Some(2), false, Redraw::PREVIEW, 4),
    ] {
        let cx = context(&picker, rank);
        assert_eq!(
            interface.before_frame(&mut state, &cx, changed, deadline),
            redraw
        );
        assert_eq!(
            interface.after_frame(&mut state, &cx, deadline),
            Redraw::NONE
        );
        assert_eq!(generated.borrow().len(), count);
        let pane = pane(&interface, &state);
        assert_eq!(
            pane.buffer().line(0).unwrap().as_str(),
            *cx.hovered().unwrap().value()
        );
        assert_eq!(pane.offset(), (7, 9));
        assert!(pane.line_numbers());
        assert!(pane.is_err());
    }

    let cx = context(&picker, Some(2));
    for _ in 0..2 {
        assert_eq!(
            interface.preview_event(&mut state, PreviewEvent::Refresh, &cx),
            Redraw::NONE
        );
    }
    assert_eq!(generated.borrow().len(), 4);
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, deadline),
        Redraw::PREVIEW
    );
    assert_eq!(
        &*generated.borrow(),
        &["alpha", "beta", "alpha", "alpha", "alpha"]
    );

    let empty = context(&picker, None);
    assert_eq!(
        interface.before_frame(&mut state, &empty, true, deadline),
        Redraw::PREVIEW
    );
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    assert_eq!(
        interface.before_frame(&mut state, &empty, false, deadline),
        Redraw::NONE
    );
    assert_eq!(
        interface.preview_event(&mut state, PreviewEvent::Refresh, &empty),
        Redraw::NONE
    );
    assert_eq!(generated.borrow().len(), 5);
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, deadline),
        Redraw::PREVIEW
    );
    assert_eq!(generated.borrow().len(), 6);
}

#[test]
fn hidden_frames_retain_the_pane_and_pending_refresh() {
    let picker = picker();
    let generated = RefCell::new(Vec::new());
    let mut interface = UncachedPreview::new_single(|item: &&'static str, pane: &mut Pane| {
        generated.borrow_mut().push(*item);
        pane.push_text(item);
        pane.set_offset(3, 4);
    });
    let cx = context(&picker, Some(0));
    let mut state = interface.init(&cx);
    let deadline = Instant::now();
    assert_eq!(
        interface.before_frame(&mut state, &cx, true, deadline),
        Redraw::PREVIEW
    );
    interface.preview_resize(&mut state, None);
    for rank in [Some(1), None, Some(2), Some(0)] {
        let hidden = context(&picker, rank).with_preview_enabled(false);
        assert_eq!(
            interface.before_frame(&mut state, &hidden, true, deadline),
            Redraw::NONE
        );
        assert_eq!(
            interface.after_frame(&mut state, &hidden, deadline),
            Redraw::NONE
        );
        assert_eq!(pane(&interface, &state).offset(), (3, 4));
    }
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, deadline),
        Redraw::NONE
    );
    assert_eq!(&*generated.borrow(), &["alpha"]);

    let _ = interface.preview_event(&mut state, PreviewEvent::Refresh, &cx);
    let hidden = context(&picker, Some(1)).with_preview_enabled(false);
    assert_eq!(
        interface.before_frame(&mut state, &hidden, false, deadline),
        Redraw::NONE
    );
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, deadline),
        Redraw::PREVIEW
    );
    assert_eq!(&*generated.borrow(), &["alpha", "alpha"]);

    assert_eq!(
        interface.before_frame(&mut state, &hidden, false, deadline),
        Redraw::NONE
    );
    assert_eq!(
        interface.before_frame(&mut state, &context(&picker, Some(1)), false, deadline),
        Redraw::PREVIEW,
    );
    assert_eq!(&*generated.borrow(), &["alpha", "alpha", "beta"]);
}

#[test]
fn events_target_the_buffered_identity_and_use_delivered_dimensions() {
    let picker = picker();
    let generated = Cell::new(0);
    let mut interface = UncachedPreview::new_single(|_: &&str, pane: &mut Pane| {
        generated.set(generated.get() + 1);
        pane.push_text("abcdefghijklmnop\nabcdefghijklmnop\n2\n3\n4\n5\n6\n7\n8\n9");
    });
    let cx = context(&picker, Some(0));
    let other = context(&picker, Some(1));
    let mut state = interface.init(&cx);
    let deadline = Instant::now();
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 3,
        }),
    );
    let _ = interface.before_frame(&mut state, &cx, true, deadline);

    for event in [
        PreviewEvent::Down(1),
        PreviewEvent::ToggleLineNumbers,
        PreviewEvent::Refresh,
    ] {
        assert_eq!(
            interface.preview_event(&mut state, event, &other),
            Redraw::NONE
        );
    }
    assert_eq!(pane(&interface, &state).offset(), (0, 0));
    assert!(!pane(&interface, &state).line_numbers());
    assert_eq!(
        interface.before_frame(&mut state, &cx, false, deadline),
        Redraw::NONE
    );
    assert_eq!(generated.get(), 1);

    for (event, offset, numbers, redraw) in [
        (PreviewEvent::Right(4), (4, 0), false, Redraw::PREVIEW),
        (PreviewEvent::PageDown(1), (4, 3), false, Redraw::PREVIEW),
        (
            PreviewEvent::Down(usize::MAX),
            (4, 7),
            false,
            Redraw::PREVIEW,
        ),
        (PreviewEvent::Down(1), (4, 7), false, Redraw::NONE),
        (
            PreviewEvent::ToggleLineNumbers,
            (4, 7),
            true,
            Redraw::PREVIEW,
        ),
        (
            PreviewEvent::SetLineNumbers(true),
            (4, 7),
            true,
            Redraw::NONE,
        ),
        (PreviewEvent::Up(1), (4, 6), true, Redraw::PREVIEW),
        (PreviewEvent::PageUp(1), (4, 3), true, Redraw::PREVIEW),
        (PreviewEvent::AlignTop, (4, 0), true, Redraw::PREVIEW),
        (PreviewEvent::AlignRight, (11, 0), true, Redraw::PREVIEW),
        (PreviewEvent::Left(2), (9, 0), true, Redraw::PREVIEW),
        (PreviewEvent::AlignLeft, (0, 0), true, Redraw::PREVIEW),
        (PreviewEvent::AlignBottom, (0, 7), true, Redraw::PREVIEW),
    ] {
        assert_eq!(interface.preview_event(&mut state, event, &cx), redraw);
        let pane = pane(&interface, &state);
        assert_eq!(pane.offset(), offset);
        assert_eq!(pane.line_numbers(), numbers);
    }
    assert_eq!(generated.get(), 1);

    let _ = interface.preview_event(&mut state, PreviewEvent::AlignTop, &cx);
    let _ = interface.preview_event(&mut state, PreviewEvent::Right(5), &cx);
    let _ = interface.preview_event(&mut state, PreviewEvent::AlignBottom, &cx);
    for size in [
        None,
        Some(Dimensions {
            width: 8,
            height: 0,
        }),
    ] {
        interface.preview_resize(&mut state, size);
        assert_eq!(pane(&interface, &state).offset(), (5, 7));
        assert_eq!(
            interface.preview_event(&mut state, PreviewEvent::PageUp(1), &cx),
            Redraw::NONE
        );
    }
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 8,
            height: 6,
        }),
    );
    assert_eq!(pane(&interface, &state).offset(), (5, 4));
    assert_eq!(
        interface.preview_event(&mut state, PreviewEvent::PageUp(1), &cx),
        Redraw::PREVIEW
    );
    assert_eq!(pane(&interface, &state).offset(), (5, 0));
    interface.preview_resize(
        &mut state,
        Some(Dimensions {
            width: 0,
            height: 6,
        }),
    );
    assert_eq!(
        interface.preview_event(&mut state, PreviewEvent::Right(1), &cx),
        Redraw::NONE
    );
}

#[test]
fn restart_invalidates_content_and_reused_indices() {
    let mut picker = picker();
    let generated = Cell::new(0);
    let mut interface = UncachedPreview::new_single(|_: &&str, pane: &mut Pane| {
        generated.set(generated.get() + 1);
        pane.push_text("content");
    });
    let cx = context(&picker, Some(0));
    let old_id = cx.hovered().unwrap().id();
    let mut state = interface.init(&cx);
    let deadline = Instant::now();
    let _ = interface.before_frame(&mut state, &cx, true, deadline);
    interface.on_restart(&mut state, &cx);
    assert!(matches!(interface.preview_content(&state), Content::Empty));
    picker.restart();
    picker.push_batch(["alpha"]);
    picker.filter().unwrap();
    let cx = context(&picker, Some(0));
    assert_ne!(cx.hovered().unwrap().id(), old_id);
    assert_eq!(
        interface.before_frame(&mut state, &cx, true, deadline),
        Redraw::PREVIEW
    );
    assert_eq!(generated.get(), 2);
}

#[test]
fn keybindings_preserve_base_overrides_and_add_preview_fallbacks() {
    use crate::event::MatchListEvent;
    use crossterm::event::{KeyCode, KeyModifiers};

    struct Base;
    impl Interface<&'static str> for Base {
        type State = ();
        type Output<'items> = Option<&'items &'static str>;

        fn init(&mut self, _: &Context<'_, &'static str>) {}

        fn finish<'items>(
            &mut self,
            _: (),
            _: Exit,
            _: Context<'items, &'static str>,
        ) -> Self::Output<'items> {
            None
        }

        fn keybind_default<A>(key: KeyEvent) -> Option<Event<A>> {
            if key == KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL) {
                Some(Event::Quit)
            } else {
                <Multi as Interface<&str>>::keybind_default(key)
            }
        }
    }

    type Adapter = UncachedPreview<Base, fn(&&str, &mut Pane)>;
    assert!(matches!(
        <Adapter as Interface<&str>>::keybind_default::<()>(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        )),
        Some(Event::Quit),
    ));
    assert!(matches!(
        <Adapter as Interface<&str>>::keybind_default::<()>(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::SHIFT,
        )),
        Some(Event::Preview(PreviewEvent::Down(1))),
    ));
    assert!(matches!(
        <Adapter as Interface<&str>>::keybind_default::<()>(KeyEvent::new(
            KeyCode::Tab,
            KeyModifiers::NONE,
        )),
        Some(Event::MatchList(MatchListEvent::ToggleDown(1))),
    ));
}
