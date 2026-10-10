use super::*;
use crate::{
    Picker,
    preview::{cache::RequestState, pane::tests::support::picker},
    render::StrRenderer,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn request<T: Send + Sync + 'static, R>(
    picker: &Picker<T, R>,
    index: u32,
) -> PreviewRequest<'_, T> {
    PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 0,
        snapshot: picker.engine.snapshot(),
        id: picker.engine.item_id(index),
    }
}

#[test]
fn request_transitions_preserve_identity_and_publication() {
    let picker = picker(["alpha"]);
    let request = request(&picker, 0);
    let id = request.id();
    let item = request.item();
    let (pending, queued) = request.defer();
    assert_eq!(queued.id(), id);
    assert!(std::ptr::eq(queued.item(), item));
    let active = queued.start().unwrap();
    assert_eq!(active.id(), id);
    assert!(std::ptr::eq(active.item(), item));
    let mut buffer = PreviewBuffer::new();
    buffer.push_str(active.item());
    assert!(active.publish(&mut buffer).is_ok());
    assert_eq!(buffer.lines().len(), 1);
    assert_eq!(buffer.line(0).unwrap().as_str(), "");
    let Ok(buffer) = RequestState::Pending(pending).try_into_buffer() else {
        panic!("expected published preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
}

#[test]
fn failed_publication_preserves_item_id_and_buffer() {
    for publish_before_restart in [false, true] {
        let mut picker = Picker::new(StrRenderer);
        picker.push_batch([String::from("alpha")]);
        while picker.engine.update(5).matching {}
        let request = request(&picker, 0);
        let id = request.id();
        let item = request.item() as *const String;
        let (pending, queued) = request.defer();
        let active = queued.start().unwrap();
        let mut buffer = PreviewBuffer::new();
        buffer.push_text("alpha\npreview");
        buffer.set_err(true);
        buffer.set_line_numbers(true);
        let allocation = buffer.line(0).unwrap().as_str().as_ptr();
        drop(RequestState::Pending(pending).into_buffer());
        assert!(active.is_cancelled());
        let cancelled = if publish_before_restart {
            let cancelled = active.publish(&mut buffer).unwrap_err();
            picker.restart();
            drop(picker);
            cancelled
        } else {
            picker.restart();
            drop(picker);
            active.publish(&mut buffer).unwrap_err()
        };
        assert_eq!(cancelled.id(), id);
        assert_eq!(cancelled.item(), "alpha");
        assert!(std::ptr::eq(cancelled.item(), item));
        assert_eq!(buffer.lines().len(), 2);
        assert_eq!(buffer.line(0).unwrap().as_str(), "alpha");
        assert_eq!(buffer.line(0).unwrap().as_str().as_ptr(), allocation);
        assert_eq!(buffer.line(1).unwrap().as_str(), "preview");
        assert!(buffer.is_err());
        assert!(buffer.line_numbers());
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
    crate::preview::pane::tests::support::settle(&mut picker);
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

    fn assert_traits<T: std::error::Error + Send + Sync>() {}
    assert_traits::<CancelledPreviewRequest<Item>>();

    for cancel_before_restart in [false, true] {
        let dropped = Arc::new(AtomicUsize::new(0));
        let mut picker = Picker::new(StrRenderer);
        picker.push_batch([Item {
            dropped: Arc::clone(&dropped),
        }]);
        while picker.engine.update(5).matching {}
        let original = request(&picker, 0);
        let id = original.id();
        let (pending, queued) = original.defer();
        let active = queued.start().unwrap();
        assert_eq!(active.id(), id);
        drop(RequestState::Pending(pending).into_buffer());
        assert!(active.is_cancelled());
        drop(active);

        let (pending, queued) = request(&picker, 0).defer();
        let cancelled = if cancel_before_restart {
            drop(RequestState::Pending(pending).into_buffer());
            let cancelled = queued.start().err().unwrap();
            picker.restart();
            drop(picker);
            cancelled
        } else {
            picker.restart();
            drop(picker);
            drop(RequestState::Pending(pending).into_buffer());
            queued.start().err().unwrap()
        };
        assert_eq!(cancelled.id(), id);
        assert!(Arc::ptr_eq(&cancelled.item().dropped, &dropped));
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
        assert_eq!(cancelled.to_string(), "preview request was cancelled");
        assert!(format!("{cancelled:?}").contains("CancelledPreviewRequest"));
        drop(cancelled);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
    }
}
