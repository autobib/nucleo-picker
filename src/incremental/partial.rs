/// The result stepping an [`Iterator`] with a limit on the size of the element returned.
#[derive(Debug, PartialEq)]
pub struct Partial {
    /// The total amount returned.
    pub size: u16,
    /// Whether the amount returned corresponds to a new element.
    pub new: bool,
}

/// An iterator adaptor which supports a variant of `next` which receives a bound, and only
/// returns the part of the value that fits within the bound.
///
/// If the value does not fit within the bound, the excess is retained and returned on subsequent
/// calls.
pub struct IncrementalIterator<I: Iterator<Item = usize>> {
    iter: I,
    partial: usize,
}

impl<I: Iterator<Item = usize>> IncrementalIterator<I> {
    /// Returns a new [`IncrementalIterator`], consuming the given iterator.
    #[inline]
    pub fn new<J: IntoIterator<IntoIter = I>>(iter: J) -> Self {
        Self {
            iter: iter.into_iter(),
            partial: 0,
        }
    }

    /// Returns whether or not the next call to [`next_partial`](Self::next_partial) will
    /// yield a [`Partial`] with `new = false`; that is, the previously returned size is
    /// incomplete.
    ///
    /// If this method returns false, [`next_partial`](Self::next_partial) could either return
    /// `None` or a [`Partial`] with `new = false` if called with `limit = 0`. If the internal
    /// iterator is not finished and `limit > 0`, the next call will return a [`Partial`] with
    /// `new = true`.
    #[inline]
    pub fn is_incomplete(&self) -> bool {
        self.partial > 0
    }

    /// Return the next [`Partial`] constrained by the provided limit.
    ///
    /// # API Guarantees
    /// 1. The returned [`Partial`] contains a `size` that is bounded above by `limit`.
    /// 2. The first returned value from a newly constructed [`IncrementalIterator`] is
    ///    either `None`, or a [`Partial`] with `new == true`.
    #[inline]
    pub fn next_partial(&mut self, limit: u16) -> Option<Partial> {
        if self.partial > 0 {
            Some(Partial {
                new: false,
                size: if self.partial > limit.into() {
                    // SAFETY: partial > limit
                    self.partial = unsafe { self.partial.unchecked_sub(limit as usize) };
                    // SAFETY: Guarantee 2: returns limit
                    limit
                } else {
                    let ret = self.partial as u16;
                    self.partial = 0;
                    // SAFETY: Guarantee 2: self.partial <= limit from branch
                    ret
                },
            })
        } else {
            // SAFETY: Guarantee 1: a newly initialized IncrementalIterator has `partial == 0`, so
            // the first iteration must reach this branch.
            match self.iter.next() {
                Some(new) => Some(Partial {
                    new: true,
                    size: if new > limit.into() {
                        // SAFETY: new > limit
                        self.partial = unsafe { new.unchecked_sub(limit as usize) };
                        // SAFETY: Guarantee 2: returns limit
                        limit
                    } else {
                        // SAFETY: Guarantee 2: new <= limit from branch
                        new as u16
                    },
                }),
                None => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct PartialTester<I: Iterator<Item = usize>> {
        partial: IncrementalIterator<I>,
    }

    impl<I: Iterator<Item = usize>> PartialTester<I> {
        fn assert(&mut self, limit: u16, size: u16, new: bool) {
            assert_eq!(
                self.partial.next_partial(limit),
                Some(Partial { size, new })
            );
        }
    }

    #[test]
    fn test_partial_iterator() {
        let mut ap = PartialTester {
            partial: IncrementalIterator::new([1, 7, 3, 2, 5]),
        };

        ap.assert(2, 1, true);
        ap.assert(5, 5, true);
        assert!(ap.partial.is_incomplete());
        ap.assert(1, 1, false);
        assert!(ap.partial.is_incomplete());
        ap.assert(1, 1, false);
        ap.assert(3, 3, true);
        ap.assert(1, 1, true);
        assert!(ap.partial.is_incomplete());
        ap.assert(8, 1, false);
        ap.assert(4, 4, true);
        ap.assert(0, 0, false);
        ap.assert(1, 1, false);
        assert!(ap.partial.next_partial(0).is_none());
    }
}
