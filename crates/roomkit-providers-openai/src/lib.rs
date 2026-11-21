//! The `OpenAI` model provider of `RoomKit` Providers.
//!
//! - [`OpenAiAiProvider`]: a provider for `OpenAI`'s Chat Completions and
//!   for the servers that speak it behind a base URL, Python's
//!   `OpenAIAIProvider`. It answers a request whole; its streams are not
//!   written yet.
//! - [`OpenAiConfig`]: what the provider is configured with, the
//!   `OpenAIConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_chat_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case of a whole answer through a local fake of the
//! endpoint, and name the few the provider answers otherwise, with the
//! reason. The crate's README lists them.
//!
//! It depends on neither Python nor `RoomKit`.

#![forbid(unsafe_code)]

mod answer;
mod config;
mod endpoint;
mod failure;
mod messages;
mod models;
mod names;
#[cfg(test)]
mod ported;
mod provider;
#[cfg(test)]
mod replay;
mod request;
mod retry;

pub use config::OpenAiConfig;
pub use provider::OpenAiAiProvider;
