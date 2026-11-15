use super::*;
use crate::{Picker, preview::lock::Poll, render::StrRenderer, session::Context};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn request<T: Send + Sync + 'static, R>(picker: &Picker<T, R>, rank: u32) -> Request<'_, T> {
    let matches = picker.matches();
    Request {
        buffer: Buffer::new(),
        epoch: 0,
        matches,
        id: matches.get(rank as usize).unwrap().id(),
    }
}

fn picker(items: impl IntoIterator<Item = &'static str>) -> Picker<&'static str, StrRenderer> {
    let mut picker = Picker::new(StrRenderer);
    picker.push_batch(items);
    picker.filter().unwrap();
    picker
}

#[test]
fn request_transitions_preserve_identity_and_publication() {
    let picker = picker(["alpha"]);
    let mut request = request(&picker, 0);
    request.buffer.push_str("reusable request storage");
    let reusable = request.buffer.line(0).unwrap().as_str().as_ptr();
    request.buffer.clear();
    let item = request.item_ref();
    let id = item.id();
    assert_eq!(request.id(), id);
    assert!(std::ptr::eq(request.item(), item.value()));
    let cx = Context::new(picker.matches()).with_rank(Some(0));
    let hovered = cx.hovered().unwrap();
    assert_eq!(hovered.id(), id);
    assert!(std::ptr::eq(hovered.value(), item.value()));
    let (pending, queued) = request.defer();
    assert_eq!(queued.id(), id);
    assert_eq!(queued.item_ref().id(), id);
    assert!(std::ptr::eq(queued.item_ref().value(), item.value()));
    let active = queued.start().unwrap();
    assert_eq!(active.id(), id);
    assert_eq!(active.item_ref().id(), id);
    assert!(std::ptr::eq(active.item_ref().value(), item.value()));
    let mut buffer = Buffer::new();
    buffer.push_str(active.item());
    let allocation = buffer.line(0).unwrap().as_str().as_ptr();
    let mut pane = buffer
        .into_pane()
        .with_offset(11, 23)
        .with_line_numbers(true);
    pane.set_err(true);
    assert!(active.publish(&mut pane).is_ok());
    assert_eq!(pane.buffer().lines().len(), 1);
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "");
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), reusable);
    assert_eq!(pane.offset(), (0, 0));
    assert!(!pane.line_numbers());
    assert!(!pane.is_err());
    let Poll::Ready(pane) = pending.reader.poll() else {
        panic!("expected published preview");
    };
    assert_eq!(pane.buffer().line(0).unwrap().as_str(), "alpha");
    assert_eq!(pane.buffer().line(0).unwrap().as_str().as_ptr(), allocation);
    assert_eq!(pane.offset(), (11, 23));
    assert!(pane.line_numbers());
    assert!(pane.is_err());
}

#[test]
fn failed_publication_preserves_item_id_and_buffer() {
    for publish_before_restart in [false, true] {
        let mut picker = Picker::new(StrRenderer);
        picker.push_batch([String::from("alpha")]);
        picker.filter().unwrap();
        let request = request(&picker, 0);
        let id = request.id();
        let item = request.item() as *const String;
        let (pending, queued) = request.defer();
        let active = queued.start().unwrap();
        let mut buffer = Buffer::new();
        buffer.push_text("alpha\npreview");
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();
        let mut pane = buffer
            .into_pane()
            .with_offset(17, 29)
            .with_line_numbers(true);
        pane.set_err(true);
        drop(pending.reader.cancel_any());
        assert!(active.is_cancelled());
        let cancelled = if publish_before_restart {
            let cancelled = active.publish(&mut pane).unwrap_err();
            picker.restart();
            drop(picker);
            cancelled
        } else {
            picker.restart();
            drop(picker);
            active.publish(&mut pane).unwrap_err()
        };
        assert_eq!(cancelled.id(), id);
        assert_eq!(cancelled.item_ref().id(), id);
        assert_eq!(cancelled.item(), "alpha");
        assert!(std::ptr::eq(cancelled.item_ref().value(), item));
        assert_eq!(pane.offset(), (17, 29));
        assert!(pane.line_numbers());
        assert!(pane.is_err());
        let buffer = pane.buffer();
        assert_eq!(buffer.lines().len(), 2);
        assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
        assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);
        assert_eq!(buffer.line(1).unwrap().as_str(), "preview");
    }
}

#[test]
fn ids_distinguish_duplicate_items_and_survive_appends() {
    let mut picker = picker(["same", "same"]);
    let first = request(&picker, 0).id();
    let second = request(&picker, 1).id();
    let mut map = HashMap::from([(first, "first"), (second, "second")]);
    assert_eq!(map.len(), 2);
    picker.push_batch(["same"]);
    picker.filter().unwrap();
    assert_eq!(map[&request(&picker, 0).id()], "first");
    assert_eq!(map[&request(&picker, 1).id()], "second");
    let third = request(&picker, 2).id();
    assert!(map.insert(third, "third").is_none());
    assert_eq!(BTreeSet::from([first, second, third]).len(), 3);
}

#[test]
fn cancelled_request_retains_item_and_id_after_restart_and_picker_drop() {
    struct Item {
        dropped: Arc<AtomicUsize>,
    }

    impl AsRef<str> for Item {
        fn as_ref(&self) -> &str {
            "alpha"
        }
    }

    impl Drop for Item {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    for cancel_before_restart in [false, true] {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut picker = Picker::new(StrRenderer);
        picker.push_batch([Item {
            dropped: Arc::clone(&dropped),
        }]);
        picker.filter().unwrap();
        let original = request(&picker, 0);
        let id = original.id();
        let (pending, queued) = original.defer();
        let active = queued.start().unwrap();
        assert_eq!(active.id(), id);
        drop(pending.reader.cancel_any());
        assert!(active.is_cancelled());
        drop(active);

        let (pending, queued) = request(&picker, 0).defer();
        let cancelled = if cancel_before_restart {
            drop(pending.reader.cancel_any());
            let cancelled = queued.start().err().unwrap();
            picker.restart();
            drop(picker);
            cancelled
        } else {
            picker.restart();
            drop(picker);
            drop(pending.reader.cancel_any());
            queued.start().err().unwrap()
        };
        assert_eq!(cancelled.id(), id);
        assert_eq!(cancelled.item_ref().id(), id);
        assert!(Arc::ptr_eq(&cancelled.item_ref().value().dropped, &dropped));
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        drop(cancelled);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }
}
