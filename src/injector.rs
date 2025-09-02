use std::sync::Arc;

use nucleo as nc;

use super::Render;

/// A handle which allows adding new items to a [`Picker`](super::Picker).
///
/// This struct is cheaply clonable and can be sent across threads. By default, add new items to
/// the [`Picker`](super::Picker) using the [`push`](Injector::push) method. For convenience, an
/// injector also implements [`Extend`] if you want to add items from an iterator.
///
/// ## `DeserializeSeed` implementation
/// If your items are being read from an external source and deserialized within the
/// [`serde`](::serde) framework, you may find it convenient to enable the `serde` optional feature.
/// With this feature enabled, an injector implements
/// [`DeserializeSeed`](::serde::de::DeserializeSeed) and expects a sequence of picker items.
/// The [`DeserializeSeed`](::serde::de::DeserializeSeed) implementation sends the items to the
/// picker immediately, without waiting for the entire file to be deserialized (or even loaded into
/// memory).
/// ```
/// use nucleo_picker::{render::StrRenderer, Picker, Render};
/// use serde::{de::DeserializeSeed, Deserialize};
/// use serde_json::Deserializer;
///
/// let input = r#"
///   [
///    "Alvar Aalto",
///    "Frank Lloyd Wright",
///    "Zaha Hadid",
///    "Le Corbusier"
///   ]
/// "#;
///
/// // the type annotation here also tells `serde_json` to deserialize `input` as a sequence of
/// // `String`.
/// let mut picker: Picker<String, _> = Picker::new(StrRenderer);
/// let injector = picker.injector();
///
/// // in practice, you would read from a file or a socket and use
/// // `Deserializer::from_reader` instead, and run this in a separate thread
/// injector
///     .deserialize(&mut Deserializer::from_str(input))
///     .unwrap();
/// ```
pub struct Injector<T, R> {
    inner: nc::Injector<T>,
    render: Arc<R>,
}

impl<T, R> Clone for Injector<T, R> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            render: self.render.clone(),
        }
    }
}

impl<T: Send + Sync + 'static, R: Render<T>> Injector<T, R> {
    pub(crate) fn new(inner: nc::Injector<T>, render: Arc<R>) -> Self {
        Self { inner, render }
    }
}

impl<T, R: Render<T>> Injector<T, R> {
    /// Add an item to the picker.
    pub fn push(&self, item: T) {
        self.inner.push(item, |s, columns| {
            columns[0] = self.render.render(s).as_ref().into();
        });
    }

    /// Returns a reference to the renderer internal to the picker.
    pub fn renderer(&self) -> &R {
        &self.render
    }
}

impl<T, R: Render<T>> Extend<T> for Injector<T, R> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for it in iter {
            self.push(it);
        }
    }
}

#[cfg(feature = "serde")]
mod serde {
    use serde::{
        Deserialize,
        de::{DeserializeSeed, Deserializer, SeqAccess, Visitor},
    };

    use super::Injector;
    use crate::Render;

    impl<'de, T, R> Visitor<'de> for &Injector<T, R>
    where
        T: Send + Sync + 'static + Deserialize<'de>,
        R: Render<T>,
    {
        type Value = ();

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a sequence of picker items")
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<(), S::Error>
        where
            S: SeqAccess<'de>,
        {
            while let Some(item) = seq.next_element()? {
                self.push(item);
            }

            Ok(())
        }
    }

    impl<'de, T, R> DeserializeSeed<'de> for &Injector<T, R>
    where
        T: Send + Sync + 'static + Deserialize<'de>,
        R: Render<T>,
    {
        type Value = ();

        /// Deserialize from a sequence of picker items.
        /// This implementation is enabled using the `serde` feature.
        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_seq(self)
        }
    }
}
