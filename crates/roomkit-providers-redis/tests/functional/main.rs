//! The backend on a Redis server: what a process reads of another, and what
//! the server keeps. Each test runs on the server `REDIS_URL` names and
//! skips itself, saying so, without it (`make redis-test`).
#![expect(clippy::unwrap_used, reason = "a test fails on a broken fixture")]

mod keys;
mod ports;
mod room_turn;
mod support;
