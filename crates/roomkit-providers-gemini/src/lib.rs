//! The Gemini model provider of `RoomKit` Providers.
//!
//! - [`Gemini`]: a provider for Google's Gemini models on the Developer
//!   API, Python's `GeminiAIProvider`. It answers a request whole or as a
//!   stream, of events or of text, and lists the models its endpoint
//!   serves.
//! - [`GeminiConfig`]: what the provider is configured with, the
//!   `GeminiConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form.
//!
//! Every generation is asked as a stream, a whole answer too. A stream is
//! pulled: nothing is sent before it is polled, and dropping it stops the
//! request and closes its connection.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_gemini_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case through a local fake of the endpoint, and name the
//! ones the provider answers otherwise, with the reason.
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
#[cfg(test)]
mod replay;
mod request;
mod stream;

pub use config::GeminiConfig;
pub use provider::Gemini;
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use provider::ReplayWaits;
pub use roomkit_providers_client::{TokenError, TokenFuture, TokenSource};

#[cfg(test)]
mod oauth_tests;
