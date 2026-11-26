# roomkit-providers-postgres

RoomKit's conversation store, room lock and knowledge source on Postgres, on
the schema of RoomKit's Python `PostgresStore`, so that processes of both
implementations share one database.

| Name | What it is |
| --- | --- |
| `PostgresStore` | The conversation store (RFC §14), as RoomKit's Python `PostgresStore` |
| `PostgresAdvisoryLockManager` | The room lock across processes, on Python's advisory keys |
| `PostgresKnowledgeSource` | A full-text keyword search for an AI channel's memory (RFC §20.2) |
| `PostgresStoreOptions`, `PostgresAdvisoryLockManagerOptions`, `InitOptions` | Their options and their pools' sizes |
| `POSTGRES_SCHEMA`, `POSTGRES_SCHEMA_VERSION` | The schema batch, Python's byte for byte |

The store passes `roomkit::testing::check_conversation_store`, the store
contract of RoomKit's Rust core, in its tests.

## Use

```rust,no_run
use std::sync::Arc;

use roomkit::{RoomKit, RoomKitOptions};
use roomkit_providers_postgres::{
    InitOptions, PostgresAdvisoryLockManager, PostgresAdvisoryLockManagerOptions,
    PostgresStore, PostgresStoreOptions,
};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let dsn = "postgresql://roomkit:secret@localhost/roomkit".to_owned();
let store = PostgresStore::new(PostgresStoreOptions {
    dsn: Some(dsn.clone()),
    ..PostgresStoreOptions::default()
});
store.init(InitOptions::default()).await?;
let locks = PostgresAdvisoryLockManager::new(PostgresAdvisoryLockManagerOptions {
    dsn: Some(dsn),
    ..PostgresAdvisoryLockManagerOptions::default()
});
locks.init().await?;
let kit = RoomKit::new(RoomKitOptions {
    lock_manager: Some(Arc::new(locks)),
    ..RoomKitOptions::with_store(store)
});
# drop(kit);
# Ok(())
# }
```

### PostgreSQL driver

An application that uses this crate applies these entries in the root
`Cargo.toml` of its workspace. The exact version keeps Cargo on the patched
driver when other 0.7 releases are available. The patch also aligns the SQL
types used by the application, pool and TLS connector.

```toml
[dependencies]
tokio-postgres = "=0.7.18"

[patch.crates-io]
tokio-postgres = { git = "https://github.com/sboily/rust-postgres", rev = "068bbab6cf2bd3867aa897d1e296126dfed650f8" }
postgres-types = { git = "https://github.com/sboily/rust-postgres", rev = "068bbab6cf2bd3867aa897d1e296126dfed650f8" }
postgres-protocol = { git = "https://github.com/sboily/rust-postgres", rev = "068bbab6cf2bd3867aa897d1e296126dfed650f8" }
```

This revision waits for PostgreSQL to close the cancellation connection before
the store reuses a pooled connection. Keep the patch until you upgrade to a
revision that uses a released driver containing the fix.

## Example

`examples/room_on_postgres.rs` keeps a room's message in Postgres and reads
it back from a second kit on the same database:

```sh
POSTGRES_DSN=postgresql://... cargo run -p roomkit-providers-postgres --example room_on_postgres
```
