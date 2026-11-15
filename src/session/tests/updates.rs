use std::{
    cell::RefCell,
    time::{Duration, Instant},
};

#[cfg(feature = "preview")]
use super::FrameEvents;
use super::{TestEvents, TestTerminal, frame_events};
use crate::{
    Picker, PickerOptions,
    component::PreviewComponent,
    event::{Event, MatchListEvent, PromptEvent, RecvError},
    render::StrRenderer,
    session::{Context, Exit, Interface, ItemId, Single},
};

struct Update {
    matches_changed: bool,
    query: String,
    hovered: Option<ItemId>,
    matches: Vec<ItemId>,
    #[cfg(feature = "preview")]
    preview_enabled: bool,
}

struct Probe<'a, const ENABLED: bool>(&'a RefCell<Vec<Update>>);

impl<T: Send + Sync + 'static, const ENABLED: bool> Interface<T> for Probe<'_, ENABLED> {
    type State = ();
    type Output<'items> = Option<&'items T>;

    #[cfg(feature = "preview")]
    const HAS_PREVIEW: bool = ENABLED;

    fn init(&mut self, _: &Context<'_, T>) {}

    fn finish<'items>(
        &mut self,
        (): (),
        exit: Exit,
        cx: Context<'items, T>,
    ) -> Self::Output<'items> {
        Single.finish((), exit, cx)
    }
}

impl<T: Send + Sync + 'static, const ENABLED: bool> PreviewComponent<T, ()> for Probe<'_, ENABLED> {
    fn update(
        &mut self,
        _: &mut (),
        cx: &Context<'_, T>,
        matches_changed: bool,
        _: Instant,
    ) -> bool {
        self.0.borrow_mut().push(Update {
            matches_changed,
            query: cx.query().to_owned(),
            hovered: cx.hovered().map(|item| item.id()),
            matches: cx.matches().ids().collect(),
            #[cfg(feature = "preview")]
            preview_enabled: cx.preview_dimensions().is_some(),
        });
        false
    }
}

fn picker() -> Picker<&'static str, StrRenderer> {
    let mut picker = PickerOptions::new()
        .sort_results(false)
        .frame_interval(Duration::from_secs(5))
        .picker(StrRenderer);
    picker.push_batch(["alpha", "beta"]);
    picker.filter().unwrap();
    picker
}

#[test]
fn initial_idle_and_navigation_frames_notify_even_without_preview() {
    fn check<const ENABLED: bool>() {
        let mut picker = picker();
        let ids: Vec<_> = picker.matches().ids().collect();
        let updates = RefCell::new(Vec::new());
        let mut probe = Probe::<ENABLED>(&updates);
        for _ in 0..2 {
            picker
                .core
                .pick_impl(
                    frame_events([
                        Err(RecvError::Timeout),
                        Ok(Event::MatchList(MatchListEvent::Up(1))),
                        Err(RecvError::Timeout),
                        Ok(Event::MatchList(MatchListEvent::Reset)),
                        Err(RecvError::Timeout),
                        Ok(Event::Quit),
                    ]),
                    &mut TestTerminal::default(),
                    &mut probe,
                )
                .unwrap();
        }
        let updates = updates.borrow();
        assert_eq!(updates.len(), 8);
        for session in updates.as_chunks::<4>().0 {
            for (update, (changed, index)) in
                session
                    .iter()
                    .zip([(true, 0), (false, 0), (false, 1), (false, 0)])
            {
                assert_eq!(update.matches_changed, changed);
                assert_eq!(update.hovered, Some(ids[index]));
                assert_eq!(update.matches, ids);
                #[cfg(feature = "preview")]
                assert_eq!(update.preview_enabled, ENABLED);
            }
        }
    }

    check::<false>();
    #[cfg(feature = "preview")]
    check::<true>();
}

#[test]
fn restart_notifications_coalesce_and_clear_after_delivery() {
    let mut picker = picker();
    let _injectors = picker.injector_observer(false);
    let updates = RefCell::new(Vec::new());
    picker
        .core
        .pick_impl(
            frame_events([
                Err(RecvError::Timeout),
                Ok(Event::Restart),
                Ok(Event::Restart),
                Err(RecvError::Timeout),
                Err(RecvError::Timeout),
                Ok(Event::Restart),
                Err(RecvError::Timeout),
                Err(RecvError::Timeout),
                Ok(Event::Quit),
            ]),
            &mut TestTerminal::default(),
            &mut Probe::<false>(&updates),
        )
        .unwrap();
    let updates = updates.borrow();
    assert!(
        updates
            .iter()
            .map(|update| update.matches_changed)
            .eq([true, false, true, false, true, false])
    );
    assert!(updates[2..].iter().all(|update| update.matches.is_empty()));
}

#[test]
fn query_submission_and_injection_notify_only_when_matches_are_published() {
    let mut picker = picker();
    picker.core.interval = Duration::ZERO;
    let injector = picker.injector();
    let updates = RefCell::new(Vec::<Update>::new());
    let start = Instant::now();
    let mut phase = 0;
    let mut submitted = false;
    let events = TestEvents(|_| {
        assert!(start.elapsed() < Duration::from_secs(5));
        let updates = updates.borrow();
        let last = updates.last().unwrap();
        match phase {
            0 if !submitted => {
                submitted = true;
                return Ok(Event::Prompt(PromptEvent::Reset("beta".to_owned())));
            }
            0 if last.matches.len() == 1 => {
                injector.push("betamax");
                phase = 1;
            }
            1 if last.matches.len() == 2 => phase = 2,
            2 => return Ok(Event::Quit),
            _ => {}
        }
        std::thread::yield_now();
        Err(RecvError::Timeout)
    });
    picker
        .core
        .pick_impl(
            events,
            &mut TestTerminal::default(),
            &mut Probe::<false>(&updates),
        )
        .unwrap();
    let updates = updates.borrow();
    assert!(updates[0].matches_changed);
    assert!(!updates.last().unwrap().matches_changed);
    assert_eq!(updates[1].query, "beta");
    assert_eq!(
        updates
            .iter()
            .filter(|update| update.matches_changed)
            .count(),
        3
    );
    for pair in updates.windows(2) {
        assert_eq!(pair[1].matches_changed, pair[0].matches != pair[1].matches);
    }
}

#[cfg(feature = "preview")]
#[test]
fn hidden_frames_receive_match_notifications_without_replaying_them_on_reveal() {
    use crate::event::LayoutEvent;

    let mut picker = picker();
    let injector = picker.injector();
    let updates = RefCell::new(Vec::<Update>::new());
    let start = Instant::now();
    let mut phase = 0;
    let events = FrameEvents(|| {
        assert!(start.elapsed() < Duration::from_secs(5));
        match phase {
            0 | 4 => {
                phase += 1;
                Ok(Event::Layout(LayoutEvent::TogglePreview))
            }
            1 | 5 => {
                phase += 1;
                Err(RecvError::Timeout)
            }
            2 => {
                injector.push("gamma");
                phase = 3;
                Err(RecvError::Timeout)
            }
            3 => {
                if updates.borrow().last().unwrap().matches.len() == 3 {
                    phase = 4;
                }
                std::thread::yield_now();
                Err(RecvError::Timeout)
            }
            _ => Ok(Event::Quit),
        }
    });
    picker
        .core
        .pick_impl(
            events,
            &mut TestTerminal::default(),
            &mut Probe::<true>(&updates),
        )
        .unwrap();
    let updates = updates.borrow();
    assert_eq!(
        updates
            .iter()
            .filter(|update| update.matches_changed)
            .count(),
        2
    );
    assert!(updates[0].matches_changed);
    assert!(updates[0].preview_enabled);
    assert!(!updates[1].matches_changed);
    assert!(
        updates[1..updates.len() - 1]
            .iter()
            .all(|update| !update.preview_enabled)
    );
    for pair in updates.windows(2) {
        assert_eq!(pair[1].matches_changed, pair[0].matches != pair[1].matches);
    }
    let revealed = updates.last().unwrap();
    assert!(revealed.preview_enabled);
    assert!(!revealed.matches_changed);
    assert_eq!(revealed.matches.len(), 3);
}
