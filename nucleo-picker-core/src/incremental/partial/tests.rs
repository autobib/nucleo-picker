use super::*;

#[test]
fn partial_iterator_preserves_remainders_across_limits() {
    let mut partial = IncrementalIterator::new([1, 7, 3, 2, 5]);

    for (limit, size, new, incomplete_after) in [
        (2, 1, true, false),
        (5, 5, true, true),
        (1, 1, false, true),
        (1, 1, false, false),
        (3, 3, true, false),
        (1, 1, true, true),
        (8, 1, false, false),
        (4, 4, true, true),
        (0, 0, false, true),
        (1, 1, false, false),
    ] {
        assert_eq!(partial.next_partial(limit), Some(Partial { size, new }));
        assert_eq!(partial.is_incomplete(), incomplete_after);
    }
    assert!(partial.next_partial(0).is_none());
}
