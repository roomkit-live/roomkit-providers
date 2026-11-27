# roomkit-providers-sqlite

RoomKit's conversation store in one SQLite file, the file RoomKit's Python
`SQLiteStore` keeps: either implementation reads what the other wrote.

| Name | What it is |
| --- | --- |
| `SqliteStore` | The conversation store (RFC §14), as RoomKit's Python `SQLiteStore` |
| `SqliteStoreOptions` | Its file |
| `SearchEventsOptions` | A full-text search of a room's messages |
| `SqliteSchemaError` | A file whose schema the store refuses |

The store passes `roomkit::testing::check_conversation_store`, the store
contract of RoomKit's Rust core, in its tests. One process uses a file at
a time.

## Use

```rust,no_run
use roomkit::{RoomKit, RoomKitOptions};
use roomkit_providers_sqlite::{SqliteStore, SqliteStoreOptions};

let store = SqliteStore::new(SqliteStoreOptions {
    path: "rooms.db".into(),
});
let kit = RoomKit::new(RoomKitOptions::with_store(store));
```

## Example

`examples/room_on_sqlite.rs` keeps a room's message in a SQLite file and
reads it back from a second kit on the same file:

```sh
cargo run -p roomkit-providers-sqlite --example room_on_sqlite
```
