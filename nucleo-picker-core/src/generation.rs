use std::sync::atomic::{AtomicU32, Ordering};

static LAST_GENERATION: AtomicU32 = AtomicU32::new(0);

/// An opaque, globally unique picker generation.
///
/// This picker generation uniquely identifies the underlying item pool. The generation is
/// guaranteed to be distinct for different pickers and moreover for the same picker across
/// restarts.
///
/// Various types expose the generation:
///
/// - [`Model::generation`](crate::session::Model::generation)
/// - [`Matches::generation`](crate::session::Matches::generation)
/// - [`Injector::generation`](crate::Injector::generation)
/// - [`ItemId::generation`](crate::session::ItemId::generation)
/// - [`EventBatch::generation`](crate::session::EventBatch::generation)
/// - [`Session::generation`](crate::session::Session::generation)
/// - [`Context::generation`](crate::session::Context::generation)
/// - [`OwnedItem::generation`](crate::session::OwnedItem::generation)
///
/// If the two types have matching generation, they also share the same underlying item pool.
///
/// Internally, this is implemented using a globally shared atomic `u32` counter which panics on
/// exhaustion (i.e., after restarting `2^32` times, after initializing `2^32` pickers, or some
/// combination of the two).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u32);

impl Generation {
    pub(crate) fn new() -> Self {
        Self(next_generation(&LAST_GENERATION))
    }
}

fn next_generation(counter: &AtomicU32) -> u32 {
    // This counter establishes identity; the matcher synchronizes item publication.
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |generation| {
            generation.checked_add(1)
        })
        .expect("picker item generation exhausted")
        + 1
}

#[cfg(test)]
mod tests;
