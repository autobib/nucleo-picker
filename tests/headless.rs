use std::{
    num::NonZero,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use nucleo_picker::{
    Injector,
    render::StrRenderer,
    session::{Driver, DriverOptions, MatchStatus},
};

fn options() -> DriverOptions {
    DriverOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
}

#[test]
fn replacing_items_preserves_the_query_only_when_resetting_the_renderer() {
    let mut driver = options().query("^a").driver(StrRenderer);
    driver.extend(["alpha", "beta", "apricot"].map(str::to_owned));
    assert!(
        driver
            .filter()
            .unwrap()
            .iter()
            .map(|item| item.value().as_str())
            .eq(["alpha", "apricot"])
    );
    let id = driver.matches().get(0).unwrap().id();

    driver.update_query("^beta$");
    assert_eq!(driver.filter().unwrap().get(0).unwrap().value(), "beta");
    driver.reset_renderer(StrRenderer);
    assert_eq!(driver.query(), "^beta$");
    assert!(driver.matches().is_empty());
    driver.push_batch(["beta".to_owned()]);
    assert_eq!(driver.filter().unwrap().len(), 1);
    assert!(driver.matches().item(id).is_none());

    driver.restart();
    assert_eq!(driver.query(), "");
    driver.push_batch(["new".to_owned()]);
    assert_eq!(driver.filter().unwrap().get(0).unwrap().value(), "new");
}

fn update_until(
    driver: &mut Driver<&'static str, StrRenderer>,
    ready: impl Fn(MatchStatus) -> bool,
) -> MatchStatus {
    let start = Instant::now();
    loop {
        let status = driver.update_matches(Duration::ZERO);
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
        let mut driver = options().driver(StrRenderer);
        let injector = driver.injector();
        let last_injector = injector.clone();
        assert!(driver.filter().is_none());
        injector.push("first");
        drop(injector);
        assert!(driver.filter().is_none());
        assert!(driver.matches().is_empty());

        let status = update_until(&mut driver, |status| !status.matching);
        assert!(status.injecting);
        assert!(!status.is_complete());
        assert_eq!(status.matched_item_count, 1);
        assert!(driver.filter().is_none());

        let producer = thread::spawn(move || {
            if let Some(last) = last {
                last_injector.push(last);
            }
        });
        producer.join().unwrap();
        assert!(
            driver
                .filter()
                .unwrap()
                .iter()
                .map(|item| *item.value())
                .eq(["first"].into_iter().chain(last))
        );
        assert!(driver.update_matches(Duration::ZERO).is_complete());
    }
}

#[test]
fn completion_includes_input_published_during_updates() {
    let mut driver = options().driver(StrRenderer);
    let (sender, receiver) = mpsc::channel::<Injector<&'static str, StrRenderer>>();
    let producer = thread::spawn(move || {
        for injector in receiver {
            injector.push("first");
            thread::yield_now();
            injector.push("last");
        }
    });

    for _ in 0..64 {
        driver.restart();
        driver.update_query("^last$");
        sender.send(driver.injector()).unwrap();
        let status = update_until(&mut driver, |status| status.is_complete());
        assert_eq!(status.item_count, 2);
        assert!(
            driver
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
        let mut driver = options()
            .sort_results(sort_results)
            .reverse_items(reverse_items)
            .query("^ab")
            .driver(StrRenderer);
        driver.push_batch(["ablong", "ab2", "zz", "ab", "ab1"]);
        let matches = driver.filter().unwrap();
        assert_eq!(matches.len(), expected.len());
        assert!(matches.get(expected.len()).is_none());
        assert!(matches.get(usize::MAX).is_none());
        assert!(matches.iter().map(|item| *item.value()).eq(expected));

        for (rank, expected) in expected.into_iter().enumerate() {
            let item = matches.get(rank).unwrap();
            assert_eq!(*item.value(), expected);
            assert_eq!(matches.ids().nth(rank), Some(item.id()));
            assert_eq!(matches.item(item.id()).unwrap().value(), item.value());
        }

        let mut items = matches.iter();
        let mut ids = matches.ids();
        let front = items.nth(1).unwrap();
        assert_eq!(*front.value(), expected[1]);
        assert_eq!(ids.nth(1), Some(front.id()));
        let back = items.nth_back(1).unwrap();
        assert_eq!(*back.value(), expected[2]);
        assert_eq!(ids.nth_back(1), Some(back.id()));
        assert_eq!(items.len(), 0);
        assert_eq!(ids.len(), 0);
    }
}

#[test]
fn item_provenance_survives_filtering_and_owners_outlive_the_driver() {
    let mut driver = options().reverse_items(true).driver(StrRenderer);
    driver.push_batch(["first", "last"].map(str::to_owned));
    let matches = driver.filter().unwrap();
    let first = matches.get(1).unwrap().id();
    let last = matches.get(0).unwrap().id();
    assert!(first.same_generation(last));
    assert!(first < last);

    let mut other = options().driver(StrRenderer);
    other.push_batch(["other".to_owned()]);
    let foreign = other.filter().unwrap().get(0).unwrap().id();
    let mut differently_typed = options().driver(StrRenderer);
    differently_typed.push_batch(["other"]);
    let foreign_type = differently_typed.filter().unwrap().get(0).unwrap().id();
    for id in [foreign, foreign_type] {
        assert!(!first.same_generation(id));
        assert!(!matches.contains_item(id));
        assert!(matches.item(id).is_none());
        assert!(matches.owned_item(id).is_none());
    }
    assert!(!other.matches().contains_item(first));
    assert!(!differently_typed.matches().contains_item(first));

    driver.update_query("absent");
    let (checked, unchecked) = {
        let matches = driver.filter().unwrap();
        assert!(matches.is_empty());
        assert!(matches.contains_item(first));
        let borrowed = matches.item(first).unwrap();
        let checked = matches.owned_item(first).unwrap();
        // SAFETY: filtering retains the storage that issued this initialized identifier.
        let unchecked = unsafe { matches.owned_item_unchecked(first) };
        assert_eq!(borrowed.value(), "first");
        assert!(std::ptr::eq(checked.value(), borrowed.value()));
        assert!(std::ptr::eq(checked.value(), unchecked.value()));
        (checked, unchecked)
    };

    driver.restart();
    driver.push_batch(["replacement".to_owned()]);
    let matches = driver.filter().unwrap();
    let replacement = matches.get(0).unwrap().id();
    for old in [first, last] {
        assert!(!replacement.same_generation(old));
        assert!(!matches.contains_item(old));
        assert!(matches.item(old).is_none());
        assert!(matches.owned_item(old).is_none());
    }
    drop(driver);

    assert_eq!(checked.id(), first);
    assert_eq!(checked.value(), "first");
    drop(checked);
    assert_eq!(unchecked.item_ref().id(), first);
    assert_eq!(unchecked.item_ref().value(), "first");
}
