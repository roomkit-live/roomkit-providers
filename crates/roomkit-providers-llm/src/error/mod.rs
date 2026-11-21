//! What the contract fails with: a value it refuses, and a provider's call
//! that failed.
//!
//! - [`ValueError`]: a value the contract refuses.
//! - [`ProviderError`] and [`ProviderErrorOptions`]: a provider's call that
//!   failed, with [`is_context_overflow_message`] to read its words.

mod provider;
mod value;

pub use self::provider::{ProviderError, ProviderErrorOptions, is_context_overflow_message};
pub use self::value::ValueError;
