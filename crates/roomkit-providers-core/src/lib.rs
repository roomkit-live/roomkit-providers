//! Runtime-independent contracts for model providers.
//!
//! This crate is where a provider's request, response, stream events,
//! failures and model catalogue are defined, apart from any vendor. It has
//! no I/O and no async runtime, and depends on neither Python nor `RoomKit`:
//! provider implementations own their HTTP, and hosts own their
//! conversations.
//!
//! The workspace is being set up: the crate defines no item yet.

#![forbid(unsafe_code)]
