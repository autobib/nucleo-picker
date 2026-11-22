use super::{AtomicU32, next_generation};
use std::panic::catch_unwind;

#[test]
fn generation_exhaustion_never_wraps_even_after_caught_panics() {
    let counter = AtomicU32::new(u32::MAX - 1);
    assert_eq!(next_generation(&counter), u32::MAX);
    for _ in 0..2 {
        assert!(catch_unwind(|| next_generation(&counter)).is_err());
    }
}
