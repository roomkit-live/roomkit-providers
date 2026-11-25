# roomkit-providers-redis

RoomKit's turn backend across processes, on Redis.

RoomKit's core runs an agent's turn in another process than its room's
through a `DistributedTurnBackend`. That backend's logic is the core's; it
reaches the other processes through two ports a service implements:

- a `TurnBroker` carries the messages between processes, an inbox a
  process;
- a `TurnRegistry` keeps what the processes share: their beats, each room's
  holder, the presence last published.

This crate implements both on one Redis server and assembles the backend on
them:

| Name | What it is |
| --- | --- |
| `RedisTurnBackend` | The backend on Redis, as RoomKit's Python `RedisTurnBackend` |
| `RedisTurnBroker` | The broker: a stream per process's inbox |
| `RedisTurnRegistry` | The registry: sorted sets and hashes written by scripts, at the server's time |

## Use

```rust,no_run
use std::sync::Arc;

use roomkit::turns::TurnBackendHandle;
use roomkit::{RoomKit, RoomKitOptions};
use roomkit_providers_redis::{RedisTurnBackend, RedisTurnBackendOptions};

# fn main() -> Result<(), roomkit::RoomKitError> {
let backend = RedisTurnBackend::new(
    Some("redis://localhost:6379"),
    RedisTurnBackendOptions::default(),
)?;
let kit = RoomKit::new(RoomKitOptions {
    turn_backend: Some(TurnBackendHandle::new(Arc::new(backend))),
    ..Default::default()
});
# drop(kit);
# Ok(())
# }
```

The processes given the same server and the same `key_prefix` reach each
other: a room's process submits a turn, the process that serves the agent's
executor runs it. `RedisTurnBackendOptions::client` gives the backend the
application's own connection manager for its commands; the inbox and the
beat keep connections of their own, opened at the URL.

## With RoomKit's Python

The keys, the scripts and the messages are those of RoomKit's Python
`RedisTurnBackend`. A Rust process and a Python process on one server, with
one key prefix, serve each other's turns; the interop test of this crate
holds that.

## What Redis keeps

Every key starts with the backend's `key_prefix` (`roomkit:turns` by
default).

| Key | Kind | Holds |
| --- | --- | --- |
| `inbox:<node>` | stream | The messages posted to a process |
| `alive`, `executors` | sorted set, hash | Each executor's last beat and its declaration |
| `owners` | sorted set | Each room owner's last beat |
| `running:<node>`, `generation:<node>` | set, string | The turns a process runs and the count of its changes |
| `holder:<channel>:<room>` | string | The executor holding a room's state for an agent |
| `presence:<room>:<channel>` | string | The presence last published, kept a week |

A key lasts three heartbeat timeouts past its last write, so a process that
died leaves nothing behind.

## Tests

The unit tests need no service. The functional tests run on a Redis server
`REDIS_URL` names and skip themselves without it: the contract of the two
ports, as RoomKit's core states it (`roomkit::testing`), the keys and their
lifetimes, a room's turn across two processes, and the interop with
RoomKit's Python.
