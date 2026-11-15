use super::*;

fn assert_partial(
    partial: &mut IncrementalIterator<impl Iterator<Item = usize>>,
    limit: u16,
    size: u16,
    new: bool,
) {
    assert_eq!(partial.next_partial(limit), Some(Partial { size, new }));
}

#[test]
fn test_partial_iterator() {
    let mut partial = IncrementalIterator::new([1, 7, 3, 2, 5]);

    assert_partial(&mut partial, 2, 1, true);
    assert_partial(&mut partial, 5, 5, true);
    assert!(partial.is_incomplete());
    assert_partial(&mut partial, 1, 1, false);
    assert!(partial.is_incomplete());
    assert_partial(&mut partial, 1, 1, false);
    assert_partial(&mut partial, 3, 3, true);
    assert_partial(&mut partial, 1, 1, true);
    assert!(partial.is_incomplete());
    assert_partial(&mut partial, 8, 1, false);
    assert_partial(&mut partial, 4, 4, true);
    assert_partial(&mut partial, 0, 0, false);
    assert_partial(&mut partial, 1, 1, false);
    assert!(partial.next_partial(0).is_none());
}
