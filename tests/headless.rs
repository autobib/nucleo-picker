use std::{
    num::NonZero,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use nucleo_picker::{
    Injector, Picker, PickerOptions,
    render::StrRenderer,
    session::{MatchStatus, Matches},
};

fn options() -> PickerOptions {
    PickerOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
}

fn update_until(
    picker: &mut Picker<&'static str, StrRenderer>,
    ready: impl Fn(MatchStatus) -> bool,
) -> MatchStatus {
    let start = Instant::now();
    loop {
        let status = picker.update_matches(Duration::ZERO);
        if ready(status) {
            return status;
        }
        assert!(start.elapsed() < Duration::from_secs(5), "{status:?}");
        thread::yield_now();
    }
}

#[test]
fn filtering_requires_all_current_injectors_to_be_dropped() {
    for last in [None, Some("last")] {
        let mut picker = options().picker(StrRenderer);
        let injector = picker.injector();
        let last_injector = injector.clone();
        assert!(picker.filter().is_none());
        injector.push("first");
        drop(injector);
        assert!(picker.filter().is_none());
        assert!(picker.matches().is_empty());

        let status = update_until(&mut picker, |status| !status.matching);
        assert!(!status.is_complete());
        assert_eq!(status.matched_item_count, 1);
        assert!(picker.filter().is_none());

        let producer = thread::spawn(move || {
            if let Some(last) = last {
                last_injector.push(last);
            }
        });
        producer.join().unwrap();
        assert!(
            picker
                .filter()
                .unwrap()
                .iter()
                .map(|item| *item.value())
                .eq(["first"].into_iter().chain(last))
        );
        assert!(picker.update_matches(Duration::ZERO).is_complete());
    }
}

#[test]
fn completion_includes_input_published_during_updates() {
    let mut picker = options().picker(StrRenderer);
    let (sender, receiver) = mpsc::channel::<Injector<&'static str, StrRenderer>>();
    let producer = thread::spawn(move || {
        for injector in receiver {
            injector.push("first");
            thread::yield_now();
            injector.push("last");
        }
    });

    for _ in 0..64 {
        picker.restart();
        picker.update_query("^last$");
        sender.send(picker.injector()).unwrap();
        let status = update_until(&mut picker, |status| status.is_complete());
        assert_eq!(status.item_count, 2);
        assert!(
            picker
                .matches()
                .iter()
                .map(|item| *item.value())
                .eq(["last"])
        );
    }

    drop(sender);
    producer.join().unwrap();
}

#[test]
fn ranked_items_and_ids_follow_configured_match_order() {
    for (sort_results, reverse_items, expected) in [
        (false, false, ["ablong", "ab2", "ab", "ab1"]),
        (false, true, ["ab1", "ab", "ab2", "ablong"]),
        (true, false, ["ab", "ab2", "ab1", "ablong"]),
        (true, true, ["ab", "ab1", "ab2", "ablong"]),
    ] {
        let mut picker = options()
            .sort_results(sort_results)
            .reverse_items(reverse_items)
            .query("^ab")
            .picker(StrRenderer);
        picker.push_batch(["ablong", "ab2", "zz", "ab", "ab1"].map(str::to_owned));
        let matches: Matches<'_, String> = picker.filter().unwrap();
        assert_eq!(matches.len(), expected.len());
        assert!(!matches.is_empty());
        assert!(matches.get(expected.len()).is_none());
        assert!(matches.get(usize::MAX).is_none());
        assert!(
            matches
                .iter()
                .map(|item| item.value().as_str())
                .eq(expected)
        );

        for (rank, expected) in expected.into_iter().enumerate() {
            let item = matches.get(rank).unwrap();
            assert_eq!(item.value(), expected);
            assert_eq!(matches.ids().nth(rank), Some(item.id()));
            assert!(std::ptr::eq(
                matches.item(item.id()).unwrap().value(),
                item.value(),
            ));
        }

        let mut items = picker.matches().iter();
        let mut ids = picker.matches().ids();
        let front = items.nth(1).unwrap();
        assert_eq!(front.value(), expected[1]);
        assert_eq!(ids.nth(1), Some(front.id()));
        let back = items.nth_back(1).unwrap();
        assert_eq!(back.value(), expected[2]);
        assert_eq!(ids.nth_back(1), Some(back.id()));
        assert_eq!(items.len(), 0);
        assert_eq!(ids.len(), 0);
    }
}

#[test]
fn item_lookup_survives_filtering_and_owned_items_survive_restart() {
    let mut picker = options().picker(StrRenderer);
    picker.push_batch(["original", "other"].map(str::to_owned));
    let original = picker.filter().unwrap().get(0).unwrap().id();

    picker.update_query("^other$");
    let (item, owned) = {
        let matches = picker.filter().unwrap();
        assert_eq!(matches.len(), 1);
        assert!(matches.ids().all(|id| id != original));
        (
            matches.item(original).unwrap(),
            matches.owned_item(original).unwrap(),
        )
    };
    assert_eq!(item.value(), "original");
    assert!(std::ptr::eq(owned.item_ref().value(), item.value()));

    picker.restart();
    assert!(picker.matches().is_empty());
    assert!(picker.matches().item(original).is_none());
    assert!(picker.matches().owned_item(original).is_none());

    picker.push_batch(["replacement".to_owned()]);
    let matches = picker.filter().unwrap();
    let replacement = matches.get(0).unwrap();
    assert_eq!(replacement.value(), "replacement");
    assert_ne!(replacement.id(), original);
    assert!(matches.item(original).is_none());
    assert!(matches.owned_item(original).is_none());

    drop(picker);
    assert_eq!(owned.item_ref().id(), original);
    assert_eq!(owned.item_ref().value(), "original");
}
