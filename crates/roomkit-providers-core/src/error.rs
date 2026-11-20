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
}
