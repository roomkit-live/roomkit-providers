//! A response's usage as it is read: Python's `dict[str, int]`, integer
//! counters by name.

use serde::{Deserialize, Deserializer};

use crate::Metadata;
use crate::python::quoted;

/// Reads a usage, its counters in the order they were written. A counter
/// that is no integer is refused, as Python's model refuses it, in words
/// that name the counter as Python's `repr` quotes a text.
pub(crate) fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Metadata, D::Error> {
    let usage = Metadata::deserialize(deserializer)?;
    match usage
        .iter()
        .find(|(_, count)| !(count.is_i64() || count.is_u64()))
    {
        Some((name, _)) => Err(serde::de::Error::custom(format_args!(
            "usage counter {} must be an integer",
            quoted(name)
        ))),
        None => Ok(usage),
    }
}
