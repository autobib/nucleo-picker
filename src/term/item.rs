use nucleo::{Item, Utf32Str};

use crate::Render;

/// A container type since a [`Render`] implementation might return a type which needs ownership.
///
/// For the given item, check the corresponding variant. If the variant is ASCII, that means we can
/// use much more efficient ASCII processing on rendering.
pub enum Rendered<'a, S> {
    Ascii(&'a str),
    Unicode(S),
}

pub fn new_rendered<'a, T, R: Render<T>>(
    item: &Item<'a, T>,
    renderer: &R,
) -> Rendered<'a, <R as Render<T>>::Str<'a>> {
    if let Utf32Str::Ascii(bytes) = item.matcher_columns[0].slice(..) {
        Rendered::Ascii(unsafe { std::str::from_utf8_unchecked(bytes) })
    } else {
        Rendered::Unicode(renderer.render(item.data))
    }
}

impl<'a, S: AsRef<str>> AsRef<str> for Rendered<'a, S> {
    fn as_ref(&self) -> &str {
        match self {
            Rendered::Ascii(s) => s,
            Rendered::Unicode(u) => u.as_ref(),
        }
    }
}
