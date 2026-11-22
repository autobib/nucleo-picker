use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::draw::{Color, Style};

use super::*;
use crate::{Picker, preview::lock::Poll, render::StrRenderer};

pub(in crate::preview) fn request<T: Send + Sync + 'static, R>(
    picker: &Picker<T, R>,
    rank: usize,
) -> Request<'_, T> {
    let matches = picker.matches();
    Request {
        buffer: Buffer::new(),
        epoch: 0,
        matches,
        id: matches.get(rank).unwrap().id(),
    }
}

#[test]
fn deferred_requests_retain_items_and_publish_content_and_presentation() {
    let mut picker = Picker::new(StrRenderer);
    picker.push_batch([String::from("alpha")]);
    picker.filter().unwrap();
    let request = request(&picker, 0);
    let id = picker.matches().get(0).unwrap().id();
    assert_eq!(request.id(), id);
    assert_eq!(request.item_ref().id(), id);
    assert_eq!(request.item(), "alpha");
    let (pending, queued) = request.defer();
    picker.restart();
    drop(picker);

    assert_eq!(queued.id(), id);
    assert_eq!(queued.item_ref().value(), "alpha");
    let active = queued.start().unwrap();
    assert_eq!(active.id(), id);
    assert_eq!(active.item_ref().id(), id);
    let red = Style::new().fg(Color::Red);
    let mut pane = Pane::new().with_offset(11, 23).with_line_numbers(true);
    pane.push_styled_text(active.item(), red);
    pane.set_err(true);
    active.publish(&mut pane).unwrap();
    assert!(pane.buffer().lines().map(|line| line.as_str()).eq([""]));
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());

    let Poll::Ready(pane) = pending.reader.poll() else {
        panic!("expected published preview");
    };
    assert!(
        pane.buffer()
            .line(0)
            .unwrap()
            .spans()
            .eq([red.apply("alpha")])
    );
    assert_eq!(pane.offset(), (11, 23));
    assert!(pane.line_numbers());
    assert!(pane.is_err());
}

#[test]
fn cancellation_retains_item_ownership_and_leaves_unpublished_panes_intact() {
    struct Item(Arc<AtomicUsize>);

    impl AsRef<str> for Item {
        fn as_ref(&self) -> &str {
            "alpha"
        }
    }

    impl Drop for Item {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    for active in [false, true] {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut picker = Picker::new(StrRenderer);
        picker.push_batch([Item(Arc::clone(&dropped))]);
        picker.filter().unwrap();
        let original = request(&picker, 0);
        let id = original.id();
        let (pending, queued) = original.defer();
        let work = if active {
            Ok(queued.start().unwrap())
        } else {
            Err(queued)
        };
        picker.restart();
        drop(picker);
        drop(pending.reader.cancel_any());
        let cancelled = match work {
            Ok(active) => {
                assert!(active.is_cancelled());
                assert_eq!(active.item().as_ref(), "alpha");
                let mut pane = Pane::new().with_offset(17, 29).with_line_numbers(true);
                pane.push_text("alpha\npreview");
                pane.set_err(true);
                let cancelled = active.publish(&mut pane).unwrap_err();
                assert!(
                    pane.buffer()
                        .lines()
                        .map(|line| line.as_str())
                        .eq(["alpha", "preview"])
                );
                assert_eq!(pane.offset(), (17, 29));
                assert!(pane.line_numbers());
                assert!(pane.is_err());
                cancelled
            }
            Err(queued) => {
                assert!(queued.is_cancelled());
                assert_eq!(queued.item().as_ref(), "alpha");
                queued.start().err().expect("cancelled request started")
            }
        };
        assert_eq!(cancelled.id(), id);
        assert_eq!(cancelled.item_ref().id(), id);
        assert_eq!(cancelled.item().as_ref(), "alpha");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        drop(cancelled);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }
}
