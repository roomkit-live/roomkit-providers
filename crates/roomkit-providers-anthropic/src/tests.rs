//! The tests of the provider against a local fake of its endpoint, taken
//! from the crate this provider was written from: what a request carries,
//! what the provider reads of its answer, and how its failures read.
//!
//! Each test names the source test it comes from. A behaviour the replay of
//! the Python cases already holds (`crate::replay`) is not repeated here.
//! Every request goes to the local fake of `crate::replay::fake`: a provider
//! configured for Anthropic's own endpoint is pointed at it, and keeps what
//! its configuration says of its endpoint.

mod failures;
mod requests;
pub(crate) mod support;
mod transport;
