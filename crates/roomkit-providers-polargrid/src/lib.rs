//! The PolarGrid model provider of `RoomKit` Providers.
//!
//! - [`PolarGrid`]: a provider for the models of a PolarGrid edge, Python's
//!   `PolarGridAIProvider`: the pinned region's edge, or the one the
//!   autorouter picks for the model. It answers a request whole or as a
//!   stream, of events or of text, lists the models its edge serves, and
//!   says which edge it is routed to
//!   ([`PolarGrid::connected_region`]) among those PolarGrid runs
//!   ([`PolarGrid::available_regions`]).
//! - [`PolarGridConfig`]: what the provider is configured with, the
//!   `PolarGridConfig` of `RoomKit`'s Python implementation: the same names,
//!   the same defaults and the same JSON form.
//! - [`PolarGridRegion`]: an edge, Python's `PolarGridRegion`.
//!
//! A stream is pulled: nothing is sent before it is polled, and dropping it
//! stops the request and closes its connection.
//!
//! The provider is held to a table of cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_polargrid_cases.json`), each with the
//! request, the HTTP exchanges and what Python returned: the crate's tests
//! replay every case through a local fake of the autorouter and the edge,
//! and name the ones the provider answers otherwise, with the reason.
//!
//! It is built on `roomkit-providers-client`, the HTTP client every
//! provider of this repository shares, and on `RoomKit`'s Rust core for
//! what a provider is. It needs no Python.

#![forbid(unsafe_code)]

mod answer;
mod catalogue;
mod config;
mod failure;
mod provider;
#[cfg(test)]
mod replay;
mod request;
mod routing;
mod stream;

pub use catalogue::PolarGridRegion;
pub use config::PolarGridConfig;
pub use provider::PolarGrid;
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use provider::ReplayWaits;
