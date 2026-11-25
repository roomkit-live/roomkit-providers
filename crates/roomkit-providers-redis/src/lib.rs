//! RoomKit's turn backend across processes, on Redis.
//!
//! RoomKit's core runs an agent's turn in another process than its room's
//! through a `DistributedTurnBackend`, over two ports a service
//! implements: a `TurnBroker` carries the messages between processes, a
//! `TurnRegistry` keeps what they share. This crate implements both on one
//! Redis server ([`RedisTurnBroker`], [`RedisTurnRegistry`]) and assembles
//! the backend on them ([`RedisTurnBackend`]): the processes given the same
//! server and the same key prefix reach each other.
//!
//! The keys, the scripts and the messages are those of RoomKit's Python
//! `RedisTurnBackend`: a Rust process and a Python process on one server
//! serve each other's turns.
//!
//! ```no_run
//! # fn main() -> Result<(), roomkit::RoomKitError> {
//! use std::sync::Arc;
//!
//! use roomkit::turns::TurnBackendHandle;
//! use roomkit::{RoomKit, RoomKitOptions};
//! use roomkit_providers_redis::{RedisTurnBackend, RedisTurnBackendOptions};
//!
//! let backend = RedisTurnBackend::new(
//!     Some("redis://localhost:6379"),
//!     RedisTurnBackendOptions::default(),
//! )?;
//! let kit = RoomKit::new(RoomKitOptions {
//!     turn_backend: Some(TurnBackendHandle::new(Arc::new(backend))),
//!     ..Default::default()
//! });
//! # drop(kit);
//! # Ok(())
//! # }
//! ```

mod backend;
mod services;

pub use backend::{RedisTurnBackend, RedisTurnBackendOptions};
pub use services::{RedisTurnBroker, RedisTurnRegistry};
