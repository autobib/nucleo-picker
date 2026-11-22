use std::time::Instant;

use nucleo_picker_core::{
    event::MatchListEvent,
    preview::Pane,
    render::StrRenderer,
    session::{Components, Context, Exit, FrameOptions, Model, ModelOptions, Session},
};
use nucleo_picker_interface::{Interface, Single, UncachedPreview};

fn frame_options() -> FrameOptions {
    FrameOptions::new(Instant::now())
}

fn model(items: impl IntoIterator<Item = &'static str>) -> Model<String, StrRenderer> {
    let mut model = ModelOptions::new().sort_results(false).model(StrRenderer);
    model.push_batch(items.into_iter().map(str::to_owned));
    model.filter().unwrap();
    model
}

struct Update {
    matches_changed: bool,
    preview_visible: bool,
    completed: bool,
}

struct Probe<'a>(&'a mut Vec<Update>);

impl Interface<String> for Probe<'_> {
    type State = ();
    type Output<'items> = Option<&'items String>;

    fn init(&mut self, _: &Context<'_, String>) {}

    fn before_frame(
        &mut self,
        _: &mut (),
        cx: &Context<'_, String>,
        matches_changed: bool,
        _: Instant,
    ) -> Components {
        self.0.push(Update {
            matches_changed,
            preview_visible: cx.active_components().preview,
            completed: false,
        });
        Components::QUEUE
    }

    fn after_frame(&mut self, _: &mut (), _: &Context<'_, String>, _: Instant) -> Components {
        let last = self.0.last_mut().unwrap();
        assert!(!std::mem::replace(&mut last.completed, true));
        Components {
            prompt: true,
            ..Components::NONE
        }
    }

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, String>,
    ) -> Self::Output<'items> {
        Single.finish((), exit, cx)
    }
}

fn frame<I: Interface<String>>(session: &mut Session<'_, String, StrRenderer, I>) {
    session.prepare_frame(frame_options()).complete();
}

#[test]
fn preview_wrappers_forward_frame_hooks_and_combine_invalidations() {
    fn check<I: Interface<String>>(interface: I) {
        let mut model = model(["alpha", "beta"]);
        let mut session = model.session(interface);
        frame(&mut session);
        session.events().match_list(MatchListEvent::Up(1));
        let navigation = session.prepare_frame(frame_options());
        assert_eq!(navigation.redraw(), Components::ALL);
        navigation.complete();
        session.events().set_active_components(Components {
            preview: false,
            ..Components::ALL
        });
        frame(&mut session);
        let hidden = session.prepare_frame(frame_options());
        assert_eq!(
            hidden.redraw(),
            Components {
                preview: false,
                ..Components::ALL
            }
        );
        hidden.complete();
    }

    for cached in [
        false,
        #[cfg(feature = "preview-cache")]
        true,
    ] {
        let mut updates = Vec::new();
        let mut generated = 0;
        let probe = Probe(&mut updates);
        let generate = |_: &String, _: &mut Pane| generated += 1;
        match cached {
            #[cfg(feature = "preview-cache")]
            true => check(nucleo_picker_interface::CachedPreview::new(
                probe,
                nucleo_picker_interface::preview::SyncPreviewer(generate),
            )),
            _ => check(UncachedPreview::new(probe, generate)),
        }
        assert!(
            updates
                .iter()
                .map(|update| (update.matches_changed, update.preview_visible))
                .eq([(true, true), (false, true), (false, false), (false, false),])
        );
        assert!(updates.iter().all(|update| update.completed));
        assert_eq!(generated, 2);
    }
}

#[cfg(feature = "preview-cache")]
#[test]
fn buffered_navigation_targets_cached_event_items_without_generating_unseen_previews() {
    use std::cell::RefCell;

    use nucleo_picker_core::{event::PreviewEvent, preview::Content, session::Dimensions};
    use nucleo_picker_interface::{CachedPreview, preview::SyncPreviewer};

    fn ready(content: Content<'_>) -> &Pane {
        match content {
            Content::Ready(pane) => pane,
            _ => panic!("expected ready preview"),
        }
    }

    let mut model = model(["alpha", "beta", "unseen"]);
    let requested = RefCell::new(Vec::new());
    let previewer = SyncPreviewer(|item: &String, pane: &mut Pane| {
        requested.borrow_mut().push(item.clone());
        pane.push_text(item);
        for _ in 1..30 {
            pane.push_text("\n");
            pane.push_text(item);
        }
    });
    let mut session = model.session(CachedPreview::new_single(previewer));
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
    assert_eq!(*requested.borrow(), ["alpha", "beta"]);
}
