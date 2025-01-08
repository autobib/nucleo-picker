//! An incremental buffer extension implementation.

mod partial;

pub use partial::{IncrementalIterator, Partial};

pub trait OrderedCollection {
    /// Append an item to the collection.
    fn append(&mut self, item: usize);

    /// Get a mutable reference to the last element in the collection.
    ///
    /// ## Safety
    /// Must be valid if and only if there was a previous call to `append`.
    unsafe fn last_appended(&mut self) -> &mut usize;

    /// Get a slice corresponding to the current items.
    #[cfg(test)]
    fn slice(&self) -> &[usize];
}

impl OrderedCollection for &'_ mut Vec<usize> {
    fn append(&mut self, item: usize) {
        self.push(item);
    }

    unsafe fn last_appended(&mut self) -> &mut usize {
        // SAFETY: `append` was previously called.
        unsafe { self.last_mut().unwrap_unchecked() }
    }

    #[cfg(test)]
    fn slice(&self) -> &[usize] {
        self
    }
}

impl OrderedCollection for Vec<usize> {
    fn append(&mut self, item: usize) {
        self.push(item);
    }

    unsafe fn last_appended(&mut self) -> &mut usize {
        // SAFETY: `append` was previously called.
        unsafe { self.last_mut().unwrap_unchecked() }
    }

    #[cfg(test)]
    fn slice(&self) -> &[usize] {
        self
    }
}

pub trait ExtendIncremental {
    /// Extend the internal collection, ensuring not to add more than `limit_size`
    /// to the buffer in total, and not step the underlying iterator more than `limit_steps` times.
    ///
    /// Returns the total of the elements added to the buffer.
    fn extend_bounded(&mut self, limit_size: u16, limit_steps: usize) -> u16;

    /// Extend the internal collection, ensuring not to add more than `limit_size` to the
    /// buffer in total.
    ///
    /// Returns the total of the elements added to the buffer.
    fn extend_unbounded(&mut self, limit_size: u16) -> u16;
}

/// Incremental collection of an [`Iterator`] of [`usize`] into a vector.
///
/// See the [`extended_bounded`](Self::extend_bounded) method for more detail.
pub struct Incremental<C: OrderedCollection, I: Iterator<Item = usize>> {
    /// The internal vector.
    vec: C,
    /// The internal iterator.
    sizes: IncrementalIterator<I>,
}

impl<C: OrderedCollection, I: Iterator<Item = usize>> ExtendIncremental for Incremental<C, I> {
    #[inline]
    fn extend_bounded(&mut self, limit_size: u16, limit_steps: usize) -> u16 {
        self.extend_impl(limit_size, limit_steps)
    }

    fn extend_unbounded(&mut self, limit_size: u16) -> u16 {
        self.extend_impl(limit_size, ())
    }
}

impl<C: OrderedCollection, I: Iterator<Item = usize>> Incremental<C, I> {
    /// Initialize an [`IncrementalExtension`] targeting the given vector with the
    /// provided iterator.
    ///
    /// New elements will be appended to the vector.
    pub fn new(vec: C, sizes: I) -> Self {
        Self {
            vec,
            sizes: IncrementalIterator::new(sizes),
        }
    }

    #[cfg(test)]
    pub fn view(&self) -> &[usize] {
        self.vec.slice()
    }

    #[inline]
    fn extend_impl<D: Decrement>(&mut self, limit_size: u16, limit_steps: D) -> u16 {
        // SAFETY: extend_impl_inverted returns a value less than `limit_size`.
        unsafe { limit_size.unchecked_sub(self.extend_impl_inverted(limit_size, limit_steps)) }
    }

    /// The actual implementation of the 'reversed' version, which returns the number of remaining
    /// elements to be added.
    #[inline]
    fn extend_impl_inverted<D: Decrement>(
        &mut self,
        mut remaining: u16,
        mut limit_steps: D,
    ) -> u16 {
        while remaining > 0 {
            if limit_steps.is_finished() && !self.sizes.is_incomplete() {
                return remaining;
            }

            match self.sizes.next_partial(remaining) {
                Some(Partial { new, size }) => {
                    unsafe {
                        // SAFETY: `next_partial` returns a `size` which is at most `limit_size`.
                        remaining = remaining.unchecked_sub(size);
                        if new {
                            // SAFETY: we can only be in this branch if the guard call to
                            // `self.sizes.next_is_not_new()` returned false, in which
                            // if `limit_steps.is_finished()`, we would have returned earlier.
                            limit_steps.decr();
                            self.vec.append(size as usize);
                        } else {
                            // SAFETY: there must have been a previous call to `self.vec.append`
                            // since the first item returned by an `IncrementalIterator` is
                            // guaranteed to be new.
                            let buf_last = self.vec.last_appended();
                            // SAFETY: the underlying iterator yields `usize`, so the size of each
                            // element in total cannot exceed a `usize`.
                            *buf_last = buf_last.unchecked_add(size as usize);
                        }
                    }
                }
                None => {
                    return remaining;
                }
            }
        }

        0
    }
}

/// An internal trait for a counter which can be decreased until it is finished.
///
/// The implementation for [`usize`] represents a 'bounded' counter, and the implementation for
/// `()` represents an 'unbounded' counter.
trait Decrement {
    /// Whether or not we have finished decrementing this value.
    fn is_finished(&self) -> bool;

    /// Decrement the value.
    ///
    /// # Safety
    /// Can only be called if `is_finished` returned false.
    unsafe fn decr(&mut self);
}

impl Decrement for () {
    #[inline]
    fn is_finished(&self) -> bool {
        false
    }

    #[inline]
    unsafe fn decr(&mut self) {}
}

impl Decrement for usize {
    #[inline]
    fn is_finished(&self) -> bool {
        *self == 0
    }

    #[inline]
    unsafe fn decr(&mut self) {
        // SAFETY: only called if `is_finished` returned false, in which case `self >= 1`.
        unsafe { *self = self.unchecked_sub(1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_incremental() {
        let mut vec = Vec::new();
        let mut incr = Incremental::new(&mut vec, [1, 6, 2, 3, 5, 3, 5].into_iter());

        assert_eq!(incr.extend_bounded(5, 2), 5);
        assert_eq!(incr.view(), &[1, 4]);

        assert_eq!(incr.extend_bounded(2, 1), 2);
        assert_eq!(incr.view(), &[1, 6]);

        assert_eq!(incr.extend_bounded(1, 1), 1);
        assert_eq!(incr.view(), &[1, 6, 1]);

        assert_eq!(incr.extend_bounded(0, 1), 0);
        assert_eq!(incr.view(), &[1, 6, 1]);

        assert_eq!(incr.extend_bounded(10, 1), 4);
        assert_eq!(incr.view(), &[1, 6, 2, 3]);

        assert_eq!(incr.extend_bounded(2, 3), 2);
        assert_eq!(incr.view(), &[1, 6, 2, 3, 2]);

        assert_eq!(incr.extend_bounded(1, 2), 1);
        assert_eq!(incr.view(), &[1, 6, 2, 3, 3]);

        assert_eq!(incr.extend_bounded(1, 4), 1);
        assert_eq!(incr.view(), &[1, 6, 2, 3, 4]);

        assert_eq!(incr.extend_bounded(0, 0), 0);
        assert_eq!(incr.view(), &[1, 6, 2, 3, 4]);

        assert_eq!(incr.extend_bounded(100, 4), 9);
        assert_eq!(incr.view(), &[1, 6, 2, 3, 5, 3, 5]);
    }
}
