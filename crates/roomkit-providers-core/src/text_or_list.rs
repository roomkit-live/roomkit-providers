//! Python's `str | list[T]` on input: a text, or a list of `T`.

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::SeqAccessDeserializer;
use serde::de::{Deserialize, Deserializer, Error, SeqAccess, Visitor};

/// A text, or a list of `T`.
pub(crate) enum TextOrList<T> {
    /// A text alone.
    Text(String),
    /// Its items, in order.
    List(Vec<T>),
}

impl<'de, T: Deserialize<'de>> TextOrList<T> {
    /// Reads a text or a list of `T`. Anything else is refused with an
    /// error that says `expecting` was expected.
    pub(crate) fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
        expecting: &'static str,
    ) -> Result<Self, D::Error> {
        deserializer.deserialize_any(TextOrListVisitor {
            expecting,
            item: PhantomData,
        })
    }
}

struct TextOrListVisitor<T> {
    expecting: &'static str,
    item: PhantomData<fn() -> T>,
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for TextOrListVisitor<T> {
    type Value = TextOrList<T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E: Error>(self, text: &str) -> Result<Self::Value, E> {
        Ok(TextOrList::Text(text.to_owned()))
    }

    fn visit_string<E: Error>(self, text: String) -> Result<Self::Value, E> {
        Ok(TextOrList::Text(text))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<Self::Value, A::Error> {
        Vec::deserialize(SeqAccessDeserializer::new(items)).map(TextOrList::List)
    }
}
