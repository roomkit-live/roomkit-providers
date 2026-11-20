//! The contract's error for a value it refuses.

use serde_json::Value;

use crate::python::{quoted, repr};

/// A value the contract refuses. `RoomKit`'s Python implementation raises a
/// `ValueError` for each of its cases but one:
/// [`UnmadeCalls`](Self::UnmadeCalls), where it raises a `RuntimeError`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValueError {
    /// A tool's name is one no vendor accepts (RFC §6.7): an empty one, or
    /// one with a character other than a letter, a digit, `_`, `.`, `:` or
    /// `-`. The name is quoted as Python's `repr` quotes it.
    #[error(
        "tool name {} is accepted by no provider: use letters, digits, '_', '.', ':' or '-'",
        quoted(.name)
    )]
    ToolName {
        /// The name.
        name: String,
    },
    /// A model's price holds a field Python's `ModelPricing` refuses: a
    /// rate below 0 or not finite, a multiplier or a threshold not above
    /// 0, an empty currency (pydantic's words).
    #[error("ModelPricing.{field}: {reason}")]
    Pricing {
        /// The field refused.
        field: &'static str,
        /// Why, in pydantic's words.
        reason: String,
    },
    /// A response's billed usage counter is no non-negative integer, as
    /// Python's `ModelPricing.cost_for` raises `ValueError`.
    #[error("usage counter {} must be a non-negative integer", quoted(.name))]
    UsageCounter {
        /// The counter.
        name: String,
    },
    /// A calendar day that does not exist, or a text that writes none, as
    /// Python's `date` and `date.fromisoformat` raise `ValueError`.
    #[error("{reason}")]
    Date {
        /// Python's words.
        reason: String,
    },
    /// A response schema outside the portable subset (RFC §6.7), as
    /// Python's `check_portable_schema` raises `ValueError`, in its words:
    /// the path of the first subschema that is not portable, and why.
    #[error("{reason}")]
    ResponseSchema {
        /// Python's words.
        reason: String,
    },
    /// A round is asked to replay calls it never made (RFC §6.4), where
    /// Python raises `RuntimeError`, in its words.
    #[error("replaying calls the round never made: {}", listed(.ids))]
    UnmadeCalls {
        /// The ids of the calls the round never made, sorted.
        ids: Vec<String>,
    },
}

/// `ids` as Python writes a list of texts.
fn listed(ids: &[String]) -> String {
    repr(&Value::from(ids))
}
