//! A response's usage as it is read: Python's `dict[str, int]`, integer
//! counters by name.

use serde::{Deserialize, Deserializer};

use crate::Metadata;

/// Reads a usage, its counters in the order they were written. A counter
/// that is no integer is refused, as Python's model refuses it.
pub(crate) fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Metadata, D::Error> {
    let usage = Metadata::deserialize(deserializer)?;
    match usage
        .iter()
        .find(|(_, count)| !(count.is_i64() || count.is_u64()))
    {
        Some((name, _)) => Err(serde::de::Error::custom(format_args!(
            "usage counter '{name}' must be an integer"
        ))),
        None => Ok(usage),
    }
}
