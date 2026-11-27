//! The HTTP client a model provider of `RoomKit` Providers is built on:
//! what every provider that speaks HTTP does the same way, written once.
//!
//! A provider crate holds its wire: its configuration, how it writes a
//! request, how it reads an answer and the events of a stream, how it
//! reads an error body and words a failure of its own, its catalogue and
//! its recorded cases. It builds the rest from this crate:
//!
//! - [`Client`]: the HTTP client, built from the [`Settings`] a provider
//!   states. It sends a [`Call`] until an answer comes or no try is left,
//!   whole ([`Client::send`]) or as a stream of events
//!   ([`Client::stream`], [`stream::Streamed`]), and bounds what it reads
//!   of each. When it is built, and only then, it reads from the
//!   environment the proxies it sends through and the certificate
//!   authorities it checks a server against, as the client of `RoomKit`'s
//!   Python providers reads them.
//! - [`retry`]: whether a failed try is tried again and after what wait,
//!   with the [`retry::Rule`] that says where a vendor's SDK differs.
//! - [`failure`]: a [`Failure`] as a provider builds it, and the one
//!   [`Exit`] it leaves through as the contract's error, without a secret
//!   of the configuration, on one line and bounded. A provider gets its
//!   exit from its client ([`Client::exit`]), which adds the secrets it
//!   alone knows.
//! - [`endpoint`]: the URL of a path under a base URL, with the base URLs
//!   refused when a provider is built, and whether a base URL is the
//!   vendor's own endpoint.
//! - [`catalogue`]: what every provider reads the same way of the model
//!   catalogue its crate embeds, and of the page a model listing answers.
//! - [`reasoning`]: the reasoning a turn asks of its model against what a
//!   provider's configuration states.
//! - [`chat`]: what the providers of a Chat Completions wire read alike:
//!   a stream's calls folded slot by slot, a reasoning in `<think>` tags.
//!
//! Under its `harness` feature, for the tests of a provider crate, the
//! crate also holds the replay of a fixture of recorded cases through a
//! local fake of an endpoint (`harness`). Its `replay` feature is the seam
//! a replay outside a provider crate's tests drives a provider through.
//!
//! It depends on `RoomKit`'s Rust core for what a provider is asked and
//! answers, and on no Python. The crate's README says how
//! a provider crate is written on it.

#![forbid(unsafe_code)]

pub mod catalogue;
pub mod chat;
mod client;
mod credential;
pub mod endpoint;
mod exchange;
pub mod failure;
#[cfg(any(test, feature = "harness"))]
pub mod harness;
pub mod items;
mod key;
mod object_only;
mod proxy;
pub mod reasoning;
pub mod retry;
pub mod stream;
#[cfg(test)]
mod testing;

pub use self::client::{Client, Settings, Timeouts};
#[cfg(feature = "replay")]
pub use self::client::{ReplayWaits, replayed_at};
pub use self::credential::{TokenError, TokenFuture, TokenSource};
pub use self::exchange::{ANSWER_LIMIT, Call, Pace, USER_AGENT, Wait, configured_header, is_json};
pub use self::failure::{Exit, Failure, Secrets};
pub use self::key::request_api_key;
pub use self::object_only::ObjectOnly;

// What `tried_again!` logs with, in the crate that calls it.
#[doc(hidden)]
pub use tracing as __tracing;

/// The README's examples, compiled and run with the crate's documentation
/// tests: what it says of the crate's names stays true.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct Readme;
