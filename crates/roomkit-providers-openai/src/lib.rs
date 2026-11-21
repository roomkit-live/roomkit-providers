//! The `OpenAI` model provider of `RoomKit` Providers.
//!
//! The crate holds today what a provider for `OpenAI`'s Chat Completions is
//! configured with, [`OpenAiConfig`], which is the `OpenAIConfig` of
//! `RoomKit`'s Python implementation: the same names, the same defaults and
//! the same JSON form.
//!
//! The provider itself is not written yet. What it will have to send and to
//! read is: the crate's tests hold a table of cases captured on `RoomKit`'s
//! Python provider (`tests/fixtures/python_chat_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned.
//!
//! It depends on neither Python nor `RoomKit`.

#![forbid(unsafe_code)]

mod config;
mod endpoint;

pub use config::OpenAiConfig;
