use super::*;
use crate::{
    interface::Single,
    render::StrRenderer,
    session::{Driver, DriverOptions},
};
use std::{
    num::NonZero,
    panic::{AssertUnwindSafe, catch_unwind},
};

fn driver(items: impl IntoIterator<Item = &'static str>) -> Driver<&'static str, StrRenderer> {
    let mut driver = DriverOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
        .driver(StrRenderer);
    driver.push_batch(items);
    driver.filter().unwrap();
    driver
}

#[test]
fn selection_rejects_stale_and_foreign_output_identifiers() {
    for stale in [false, true] {
        let mut source = driver(["original"]);
        let other = driver(["other"]);
        let cx = Context::new(source.matches());
        let item = cx.matches().get(0).unwrap();
        let id = item.id();
        let mut multi = Multi::new();
        let mut state = multi.init(&cx);
        multi.on_queue(&mut state, QueueEvent::Toggle(item), &cx);
        if stale {
            source.restart();
            source.push_batch(["replacement"]);
            source.filter().unwrap();
        }
        let matches = if stale {
            source.matches()
        } else {
            other.matches()
        };
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                Single.finish((), Exit::Selected(Some(id)), Context::new(matches))
            }))
            .is_err()
        );
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                multi.finish(
                    MultiState::default(),
                    Exit::Selected(Some(id)),
                    Context::new(matches),
                )
            }))
            .is_err()
        );
        assert!(catch_unwind(AssertUnwindSafe(|| { state.into_selection(matches) })).is_err());
    }
}

#[test]
fn queue_rejects_mixed_generations_until_existing_items_are_cleared() {
    let source = driver(["zero", "one"]);
    let other = driver(["other"]);
    let cx = Context::new(source.matches());
    let other_cx = Context::new(other.matches());
    let local = cx.matches().get(0).unwrap();
    let foreign = other_cx.matches().get(0).unwrap();
    for (event, event_cx) in [
        (QueueEvent::Toggle(foreign), &cx),
        (QueueEvent::Toggle(local), &other_cx),
        (QueueEvent::Toggle(foreign), &other_cx),
        (QueueEvent::Matches, &other_cx),
    ] {
        let mut multi = Multi::new();
        let mut state = multi.init(&cx);
        multi.on_queue(&mut state, QueueEvent::Matches, &cx);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                multi.on_queue(&mut state, event, event_cx)
            }))
            .is_err()
        );
        assert_eq!(
            Interface::<&str>::queue_status(&multi, &state)
                .unwrap()
                .count,
            2,
        );
        assert!(
            cx.matches()
                .ids()
                .all(|id| Interface::<&str>::is_queued(&multi, &state, id))
        );
        multi.on_queue(&mut state, QueueEvent::Clear, &cx);
        multi.on_queue(&mut state, QueueEvent::Matches, &other_cx);
        let selection = state.into_selection(other_cx.matches());
        assert!(selection.into_iter().copied().eq(["other"]));
    }
}

#[test]
fn checked_and_unchecked_conversions_preserve_filtered_items_and_order() {
    for unchecked in [false, true] {
        let mut driver = driver(["zero", "one", "two", "three", "four"]);
        let cx = Context::new(driver.matches());
        let mut multi = Multi::new();
        let mut state = multi.init(&cx);
        for rank in [4, 2, 0, 3, 1] {
            assert!(multi.on_queue(
                &mut state,
                QueueEvent::Toggle(cx.matches().get(rank).unwrap()),
                &cx,
            ));
        }
        driver.update_query("absent");
        let matches = driver.filter().unwrap();
        assert!(matches.is_empty());
        let selection = if unchecked {
            // SAFETY: filtering preserves the item storage that supplied every queued ID.
            unsafe { state.into_selection_unchecked(matches) }
        } else {
            state.into_selection(matches)
        };
        assert_eq!(selection.len(), 5);
        assert!(
            selection
                .iter()
                .copied()
                .eq(["zero", "one", "two", "three", "four"])
        );
        assert!(
            selection
                .iter_selected_order()
                .copied()
                .eq(["four", "two", "zero", "three", "one"])
        );
        let mut items = selection.into_iter();
        assert_eq!(items.nth(1), Some(&"one"));
        assert_eq!(items.len(), 3);
        assert_eq!(items.nth_back(1), Some(&"three"));
        assert_eq!(items.last(), Some(&"two"));
    }
}

#[test]
fn finish_validates_only_items_in_the_effective_output() {
    let source = driver(["original"]);
    let other = driver(["other"]);
    let cx = Context::new(source.matches());
    let foreign = other.matches().get(0).unwrap().id();
    let mut multi = Multi::new();
    for quit in [false, true] {
        let mut state = multi.init(&cx);
        assert!(multi.on_queue(&mut state, QueueEvent::Matches, &cx));
        let (exit, output_cx) = if quit {
            (Exit::Quit, Context::new(other.matches()))
        } else {
            (
                Exit::Selected(Some(foreign)),
                Context::new(source.matches()),
            )
        };
        let selection = multi.finish(state, exit, output_cx);
        assert!(selection.iter().copied().eq((!quit).then_some("original")));
    }
}

#[test]
fn queue_lifecycle_preserves_selection_order_and_obeys_the_limit() {
    let mut driver = DriverOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
        .reverse_items(true)
        .driver(StrRenderer);
    driver.push_batch(["zero", "one", "two", "three"]);
    driver.filter().unwrap();
    let cx = Context::new(driver.matches());
    let toggle = |index: usize| QueueEvent::Toggle(cx.matches().get(3 - index).unwrap());
    let mut interface = Multi::new().max_selection_count(NonZero::new(2));
    let mut state = interface.init(&cx);
    for index in [3, 1] {
        assert!(interface.on_queue(&mut state, toggle(index), &cx));
    }
    assert!(!interface.on_queue(&mut state, toggle(2), &cx));
    for index in [3, 3] {
        assert!(interface.on_queue(&mut state, toggle(index), &cx));
    }

    interface = interface.max_selection_count(NonZero::new(1));
    assert!(!interface.on_queue(&mut state, QueueEvent::Matches, &cx));
    interface = interface.max_selection_count(NonZero::new(3));
    assert!(interface.on_queue(&mut state, QueueEvent::Matches, &cx));
    assert!(!interface.on_queue(&mut state, QueueEvent::Matches, &cx));
    assert_eq!(
        Interface::<&str>::queue_status(&interface, &state),
        Some(QueueStatus {
            count: 3,
            limit: NonZero::new(3),
        })
    );
    let selection = state.into_selection(cx.matches());
    assert!(
        selection
            .iter_selected_order()
            .copied()
            .eq(["one", "three", "two"])
    );
    assert!(selection.iter().copied().eq(["one", "two", "three"]));
}
