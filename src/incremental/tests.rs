use super::*;

#[test]
fn test_incremental() {
    let mut vec = Vec::new();
    let mut incr = Incremental::new(&mut vec, [1, 6, 2, 3, 5, 3, 5].into_iter());
    for (size, steps, added, expected) in [
        (5, 2, 5, &[1, 4][..]),
        (2, 1, 2, &[1, 6]),
        (1, 1, 1, &[1, 6, 1]),
        (0, 1, 0, &[1, 6, 1]),
        (10, 1, 4, &[1, 6, 2, 3]),
        (2, 3, 2, &[1, 6, 2, 3, 2]),
        (1, 2, 1, &[1, 6, 2, 3, 3]),
        (1, 4, 1, &[1, 6, 2, 3, 4]),
        (0, 0, 0, &[1, 6, 2, 3, 4]),
        (100, 4, 9, &[1, 6, 2, 3, 5, 3, 5]),
    ] {
        assert_eq!(
            incr.extend_bounded(size, steps),
            added,
            "size={size}, steps={steps}"
        );
        assert_eq!(incr.vec.as_slice(), expected);
    }
}
