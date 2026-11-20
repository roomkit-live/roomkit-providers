//! A name read from a JSON string, and from nothing else.

use std::fmt;

use serde::Deserializer;
use serde::de::{Error, Unexpected, Visitor};

/// Reads what a JSON string names: `named` gives it, `None` for a name it
/// does not know. Anything but a string is refused, where a derived enum
/// also takes an object of one key.
pub(crate) fn read_name<'de, D, T>(
    deserializer: D,
    expecting: &'static str,
    named: fn(&str) -> Option<T>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_str(NameVisitor { expecting, named })
}

struct NameVisitor<T> {
    expecting: &'static str,
    named: fn(&str) -> Option<T>,
}

impl<T> Visitor<'_> for NameVisitor<T> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.expecting)
    }

    fn visit_str<E: Error>(self, name: &str) -> Result<T, E> {
        (self.named)(name).ok_or_else(|| E::invalid_value(Unexpected::Str(name), &self))
    }
}
