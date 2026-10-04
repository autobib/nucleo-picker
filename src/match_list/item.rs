use memchr::memchr_iter;
use nucleo::{Item, Snapshot, Utf32Str};

use crate::{Render, incremental::Incremental};

/// A trait to describe items with a certain size.
pub(super) trait ItemSize {
    /// The size of the item on the screen.
    fn size(&self) -> usize;
}

/// A list of items with variable sizes.
pub(super) trait ItemList {
    /// The item type of list.
    type Item<'a>: ItemSize
    where
        Self: 'a;

    /// The total number items in the list.
    fn total(&self) -> u32;

    /// An iterator over items below the cursor, iterating downwards.
    fn lower(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items below and including the cursor, iterating downwards.
    fn lower_inclusive(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above cursor, iterating upwards.
    fn higher(&self, cursor: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;

    /// An iterator over items above and including the cursor, iterating upwards.
    fn higher_inclusive(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>>;
}

/// An automatic extension trait for an [`ItemList`].
pub(super) trait ItemListExt: ItemList {
    /// Wrap the item sizes returned by [`lower`](ItemList::lower)
    /// into a [`Incremental`].
    fn sizes_lower<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`lower_inclusive`](ItemList::lower_inclusive)
    /// into a [`Incremental`].
    fn sizes_lower_inclusive<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.lower_inclusive(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`higher`](ItemList::higher)
    /// into an [`Incremental`].
    fn sizes_higher<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher(cursor).map(|item| item.size()))
    }

    /// Wrap the item sizes returned by [`higher_inclusive`](ItemList::higher)
    /// into an [`Incremental`].
    fn sizes_higher_inclusive<'a>(
        &self,
        cursor: u32,
        vec: &'a mut Vec<usize>,
    ) -> Incremental<&'a mut Vec<usize>, impl Iterator<Item = usize>> {
        vec.clear();
        Incremental::new(vec, self.higher_inclusive(cursor).map(|item| item.size()))
    }
}

impl<B: ItemList> ItemListExt for B {}

impl<T> ItemSize for Item<'_, T> {
    fn size(&self) -> usize {
        let num_linebreaks = match self.matcher_columns[0].slice(..) {
            Utf32Str::Ascii(bytes) => memchr_iter(b'\n', bytes).count(),
            Utf32Str::Unicode(chars) => chars.iter().filter(|ch| **ch == '\n').count(),
        };
        1 + num_linebreaks
    }
}

impl<T: Send + Sync + 'static> ItemList for Snapshot<T> {
    type Item<'a>
        = Item<'a, T>
    where
        Self: 'a;

    fn total(&self) -> u32 {
        self.matched_item_count()
    }

    fn lower(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>> {
        self.matched_items(..selection).rev()
    }

    fn lower_inclusive(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>> {
        self.matched_items(..=selection).rev()
    }

    fn higher(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>> {
        // we skip the first item rather than iterate on the range `selection + 1..` in case
        // `selection + 1` is an invalid index in which case `matched_items` would panic
        self.matched_items(selection..).skip(1)
    }

    fn higher_inclusive(&self, selection: u32) -> impl DoubleEndedIterator<Item = Self::Item<'_>> {
        // we skip the first item rather than iterate on the range `selection + 1..` in case
        // `selection + 1` is an invalid index in which case `matched_items` would panic
        self.matched_items(selection..)
    }
}

/// A container type since a [`Render`] implementation might return a type which needs ownership.
///
/// For the given item, check the corresponding variant. If the variant is ASCII, that means we can
/// use much more efficient ASCII processing on rendering.
pub enum RenderedItem<'a, S> {
    Ascii(&'a str),
    Unicode(S),
}

impl<'a, S> RenderedItem<'a, S> {
    /// Initialize a new `RenderedItem` from an [`Item`] and a [`Render`] implementation.
    pub fn new<T, R>(item: &Item<'a, T>, renderer: &R) -> Self
    where
        R: Render<T, Str<'a> = S>,
    {
        if let Utf32Str::Ascii(bytes) = item.matcher_columns[0].slice(..) {
            RenderedItem::Ascii(unsafe { std::str::from_utf8_unchecked(bytes) })
        } else {
            RenderedItem::Unicode(renderer.render(item.data))
        }
    }
}

impl<S: AsRef<str>> AsRef<str> for RenderedItem<'_, S> {
    fn as_ref(&self) -> &str {
        match self {
            RenderedItem::Ascii(s) => s,
            RenderedItem::Unicode(u) => u.as_ref(),
        }
    }
}
