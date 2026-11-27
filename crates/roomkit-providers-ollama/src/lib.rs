//! The Ollama model provider of `RoomKit` Providers.
//!
//! - [`Ollama`]: a provider for the models an Ollama server holds, on its
//!   native API, Python's `OllamaAIProvider`. It answers a request whole or
//!   as a stream, of events or of text, and lists the models the server
//!   holds with their capabilities.
//! - [`OllamaConfig`]: what the provider is configured with, the
//!   `OllamaConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form. Its `think` is a [`Think`],
//!   a switch or a [`ThinkEffort`], and its `keep_alive` a [`KeepAlive`],
//!   where Python's members hold one of two kinds.
//!
//! A whole answer is asked whole, a stream as JSON lines. A stream is
//! pulled: nothing is sent before it is polled, and dropping it stops the
//! request and closes its connection.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_ollama_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case through a local fake of the server, and name the ones
//! the provider answers otherwise, with the reason.
//!
//! It is built on `roomkit-providers-client`, the HTTP client every
//! provider of this repository shares, and on `RoomKit`'s Rust core for
//! what a provider is. It needs no Python.

#![forbid(unsafe_code)]

mod config;
mod failure;
mod host;
mod images;
mod models;
mod provider;
mod reading;
#[cfg(test)]
mod replay;
mod request;
mod stream;

pub use config::{KeepAlive, OllamaConfig, Think, ThinkEffort};
pub use provider::Ollama;
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use provider::ReplayWaits;
