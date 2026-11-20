//! The contract's error for a value it refuses.

/// A value the contract refuses, where `RoomKit`'s Python implementation
/// raises a `ValueError`.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ValueError {
    /// A tool's name is one no vendor accepts (RFC §6.7): an empty one, or
    /// one with a character other than a letter, a digit, `_`, `.`, `:` or
    /// `-`.
    #[error(
        "tool name '{name}' is accepted by no provider: use letters, digits, '_', '.', ':' or '-'"
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
    #[error("usage counter '{name}' must be a non-negative integer")]
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
}
