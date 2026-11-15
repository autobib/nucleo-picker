use std::num::NonZero;

use nucleo_picker::{DriverOptions, render::StrRenderer};

fn options() -> DriverOptions {
    DriverOptions::new()
        .threads(NonZero::new(1))
        .sort_results(false)
}

#[test]
fn identifier_provenance_and_order_are_independent_of_matching() {
    let mut driver = options().reverse_items(true).driver(StrRenderer);
    driver.push_batch(["first", "middle", "last"]);
    let matches = driver.filter().unwrap();
    let first = matches.get(2).unwrap().id();
    let middle = matches.get(1).unwrap().id();
    let last = matches.get(0).unwrap().id();
    assert!(first.same_generation(first));
    assert!(first.same_generation(middle));
    assert!(middle.same_generation(last));
    assert!(first < middle);
    assert!(middle < last);

    driver.update_query("absent");
    let matches = driver.filter().unwrap();
    assert!(matches.is_empty());
    for id in [first, middle, last] {
        assert!(matches.contains_item(id));
        assert_eq!(matches.item(id).unwrap().id(), id);
    }

    let mut other = options().driver(StrRenderer);
    other.push_batch(["foreign".to_owned()]);
    let foreign = other.filter().unwrap().get(0).unwrap().id();
    assert!(!first.same_generation(foreign));
    assert!(!foreign.same_generation(first));
    assert!(!other.matches().contains_item(first));
    assert!(!driver.matches().contains_item(foreign));
    assert!(driver.matches().item(foreign).is_none());
    assert!(driver.matches().owned_item(foreign).is_none());

    driver.restart();
    for id in [first, middle, last] {
        assert!(!driver.matches().contains_item(id));
    }
    driver.push_batch(["replacement"]);
    let matches = driver.filter().unwrap();
    let replacement = matches.get(0).unwrap().id();
    assert!(matches.contains_item(replacement));
    for id in [first, middle, last] {
        assert!(!replacement.same_generation(id));
        assert!(!matches.contains_item(id));
        assert!(matches.item(id).is_none());
        assert!(matches.owned_item(id).is_none());
    }
}

#[test]
fn checked_and_unchecked_owners_retain_identity_and_value() {
    let mut driver = options().driver(StrRenderer);
    driver.push_batch(["original", "other"].map(str::to_owned));
    let id = driver.filter().unwrap().get(0).unwrap().id();

    driver.update_query("absent");
    let (checked, unchecked) = {
        let matches = driver.filter().unwrap();
        assert!(matches.is_empty());
        assert!(matches.contains_item(id));
        let borrowed = matches.item(id).unwrap();
        let checked = matches.owned_item(id).unwrap();
        // SAFETY: filtering preserves the storage that issued this initialized identifier.
        let unchecked = unsafe { matches.owned_item_unchecked(id) };
        for owned in [&checked, &unchecked] {
            assert_eq!(owned.id(), borrowed.id());
            assert_eq!(owned.item_ref().id(), owned.id());
            assert!(std::ptr::eq(owned.value(), borrowed.value()));
            assert!(std::ptr::eq(owned.item_ref().value(), owned.value()));
        }
        (checked, unchecked)
    };

    driver.restart();
    driver.push_batch(["replacement".to_owned()]);
    let matches = driver.filter().unwrap();
    assert!(!matches.contains_item(id));
    assert!(matches.owned_item(id).is_none());
    drop(driver);

    assert_eq!(checked.id(), id);
    assert_eq!(unchecked.id(), id);
    assert_eq!(checked.value(), "original");
    assert!(std::ptr::eq(checked.value(), unchecked.value()));
    drop(checked);
    assert_eq!(unchecked.item_ref().id(), id);
    assert_eq!(unchecked.item_ref().value(), "original");
}
