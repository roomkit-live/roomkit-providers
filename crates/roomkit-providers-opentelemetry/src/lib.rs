//! RoomKit's telemetry on OpenTelemetry.
//!
//! The kit reports its spans and metrics to the provider its
//! `RoomKitOptions::telemetry` names. [`OpenTelemetryProvider`] hands them to
//! an OpenTelemetry SDK tracer and meter provider, whose exporter the
//! application chooses: the spans of a room's work, a turn in another
//! process among them, show as one trace. It is RoomKit's Python
//! `roomkit.telemetry.OpenTelemetryProvider`, under the same names.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use opentelemetry_sdk::trace::SdkTracerProvider;
//! use roomkit::{RoomKit, RoomKitOptions};
//! use roomkit_providers_opentelemetry::{OpenTelemetryProvider, OpenTelemetryProviderOptions};
//!
//! let tracer_provider = SdkTracerProvider::builder().build();
//! let telemetry = OpenTelemetryProvider::new(OpenTelemetryProviderOptions {
//!     tracer_provider: Some(tracer_provider),
//!     ..OpenTelemetryProviderOptions::default()
//! });
//! let kit = RoomKit::new(RoomKitOptions {
//!     telemetry: Some(Arc::new(telemetry).into()),
//!     ..Default::default()
//! });
//! # drop(kit);
//! ```

mod provider;
#[cfg(test)]
mod tests;

pub use provider::{OpenTelemetryProvider, OpenTelemetryProviderOptions};
