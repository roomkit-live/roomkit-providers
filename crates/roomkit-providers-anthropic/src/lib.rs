//! The Anthropic model provider of `RoomKit` Providers.
//!
//! - [`Anthropic`]: a provider for Anthropic's Messages API and for the
//!   servers that speak it behind a base URL, Python's
//!   `AnthropicAIProvider`. It answers a request whole or as a stream, of
//!   events or of text, and lists the models its endpoint serves.
//! - [`AnthropicConfig`]: what the provider is configured with, the
//!   `AnthropicConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form.
//!
//! Every generation is asked as a stream, a whole answer too. A stream is
//! pulled: nothing is sent before it is polled, and dropping it stops the
//! request and closes its connection. Every size a server controls is
//! bounded, and a stream that stalls ends at the configuration's timeout:
//! [`Anthropic`] says how.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_messages_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case through a local fake of the endpoint, a stream read
//! by read, and name the ones the provider answers otherwise, with the
//! reason. The crate's README lists them.
//!
//! It is built on `roomkit-providers-client`, the HTTP client every
//! provider of this repository shares, and on `RoomKit`'s Rust core for
//! what a provider is. It needs no Python.

#![forbid(unsafe_code)]

mod config;
mod endpoint;
mod failure;
mod folding;
mod models;
mod names;
mod provider;
mod reasoning;
#[cfg(test)]
mod replay;
mod request;
mod stream;
#[cfg(test)]
mod tests;
mod whole;

pub use config::AnthropicConfig;
pub use provider::Anthropic;
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use provider::ReplayWaits;
