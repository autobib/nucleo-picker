use memchr::memchr_iter;
use nucleo::{Item, Snapshot, Utf32Str};

use super::VariableSizeBuffer;
use crate::Render;

impl<T: Send + Sync + 'static> VariableSizeBuffer for Snapshot<T> {
    type Item<'a>
        = Item<'a, T>
    where
        Self: 'a;

    fn count(&self) -> u32 {
        self.matched_item_count()
    }

    fn size(item: &Self::Item<'_>) -> usize {
        let num_linebreaks = match item.matcher_columns[0].slice(..) {
            Utf32Str::Ascii(bytes) => memchr_iter(b'\n', bytes).count(),
            Utf32Str::Unicode(chars) => {
                // TODO: there is an upstream Unicode handling issue in that windows-style newlines are
                // mapped to `\r` instead of `\n`. Therefore we count both the number of occurrences of
                // `\r` and `\n`. This handles mixed `\r\n` as well as `\n`, but returns the incorrect
                // value in the presence of free-standing carriage returns.
                chars
                    .iter()
                    .filter(|ch| **ch == '\n' || **ch == '\r')
                    .count()
            }
        };
        // SAFETY: we are adding 1 to a usize
        1 + num_linebreaks
    }

    fn below_and_including(&self, selection: u32) -> impl Iterator<Item = Self::Item<'_>> {
        self.matched_items(..=selection).rev()
    }

    fn above(&self, selection: u32) -> impl Iterator<Item = Self::Item<'_>> {
        // we skip the first item rather than iterate on the range `selection + 1..` in case
        // `selection + 1` is an invalid index in which case `matched_items` would panic
        self.matched_items(selection..).skip(1)
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
