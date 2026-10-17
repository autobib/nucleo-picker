use std::{
    num::NonZero,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use nucleo_picker::{Injector, MatchStatus, Picker, PickerOptions, render::StrRenderer};

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
                .copied()
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
        assert!(picker.matches().iter().copied().eq(["last"]));
    }

    drop(sender);
    producer.join().unwrap();
}
