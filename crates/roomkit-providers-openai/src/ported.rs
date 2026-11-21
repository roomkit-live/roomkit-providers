//! Tests ported from the crate this provider was written from: what a whole
//! answer's request carries, what the provider reads of its answer, and how
//! its failures read.
//!
//! Each test names the source test it comes from. A behaviour the replay of
//! the Python cases already holds (`crate::replay`) is not repeated here.
//! Every request goes to the local fake of `crate::replay::fake`: a provider
//! configured for `OpenAI`'s own endpoint is pointed at it, and keeps what
//! its configuration says of its endpoint.

mod answers;
mod catalogue;
mod endpoints;
mod failures;
mod reasoning;
mod requests;
mod retries;
mod support;
mod warnings;
