use std::cell::RefCell;

use super::frame_options;
use crate::{
    event::MatchListEvent,
    interface::{
        CachedPreview,
        controller::tests::support::{TestPreviewer, driver, ready},
    },
    preview::PreviewEvent,
    session::Dimensions,
};

#[test]
fn buffered_navigation_targets_cached_event_items_without_generating_unseen_previews() {
    let mut driver = driver(["alpha", "beta", "unseen"]);
    let previewer = RefCell::new(TestPreviewer {
        lines: 30,
        ..TestPreviewer::default()
    });
    let mut session = driver.session(CachedPreview::new_single(&previewer));
    let mut initial = session.prepare_frame(frame_options());
    initial.preview().resize(Dimensions {
        width: 30,
        height: 10,
    });
    initial.complete();
    session.events().match_list(MatchListEvent::Up(1));
    session.prepare_frame(frame_options()).complete();
    {
        let mut events = session.events();
        events.preview(PreviewEvent::PageDown(1));
        events.match_list(MatchListEvent::Up(1));
        events.preview(PreviewEvent::Down(5));
        events.match_list(MatchListEvent::Down(2));
        events.preview(PreviewEvent::Down(3));
    }
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(ready(frame.preview().content()).offset(), (0, 3));
    frame.complete();

    session.events().match_list(MatchListEvent::Up(1));
    let mut frame = session.prepare_frame(frame_options());
    assert_eq!(ready(frame.preview().content()).offset(), (0, 10));
    frame.complete();
    assert_eq!(previewer.borrow().requested, ["alpha", "beta"]);
}
