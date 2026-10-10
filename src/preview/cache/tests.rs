use super::*;
use crate::preview::request::{PreviewRequest, QueuedPreviewRequest};

fn polled_queued_request() -> (PendingPreview, QueuedPreviewRequest<&'static str>) {
    let mut picker = crate::Picker::new(crate::render::StrRenderer);
    picker.push_batch(["alpha"]);
    while picker.engine.update(5).matching {}
    let (pending, queued) = PreviewRequest {
        buffer: PreviewBuffer::new(),
        epoch: 1,
        snapshot: picker.engine.snapshot(),
        id: picker.engine.item_id(0),
    }
    .defer();
    let Err(BufferNotReady::Queued(RequestState::Pending(pending))) =
        RequestState::Pending(pending).try_into_buffer()
    else {
        panic!("expected queued preview");
    };
    (pending, queued)
}

#[test]
fn promotion_preserves_work_that_starts_after_polling() {
    let (pending, queued) = polled_queued_request();
    let active = queued.start().unwrap();
    let RequestState::Pending(pending) = pending.reprioritize().unwrap() else {
        panic!("expected active preview");
    };
    assert!(!active.is_cancelled());
    let mut buffer = PreviewBuffer::new();
    assert!(active.publish(&mut buffer).is_ok());
    assert!(RequestState::Pending(pending).try_into_buffer().is_ok());
}

#[test]
fn promotion_collects_work_published_after_polling() {
    let (pending, queued) = polled_queued_request();
    let mut buffer = PreviewBuffer::new();
    buffer.push_str("completed");
    assert!(queued.start().unwrap().publish(&mut buffer).is_ok());
    let RequestState::Ready(buffer) = pending.reprioritize().unwrap() else {
        panic!("expected ready preview");
    };
    assert_eq!(buffer.line(0).unwrap().as_str(), "completed");
}

#[test]
fn promotion_reclaims_work_dropped_after_polling() {
    let (pending, queued) = polled_queued_request();
    drop(queued);
    assert!(pending.reprioritize().is_err());
}
