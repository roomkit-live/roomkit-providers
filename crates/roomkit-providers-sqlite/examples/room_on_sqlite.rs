//! A room's message kept in a SQLite file, read back after a restart.
//!
//! A first kit, on a SQLite store, takes a person's message into a room
//! and closes. A second kit, on a new store over the same file, reads the
//! room's timeline: the message is there, as the first kit stored it. The
//! example checks it, removes its file, and fails otherwise.
//!
//! ```sh
//! cargo run -p roomkit-providers-sqlite --example room_on_sqlite
//! ```
#![expect(clippy::print_stdout, reason = "an example prints what it ran")]

use std::error::Error;
use std::path::{Path, PathBuf};

use roomkit::model::{ChannelId, ChannelType, EventContent, InboundMessage, RoomId, TextContent};
use roomkit::store::ListEventsOptions;
use roomkit::testing::{RecordingTransport, RecordingTransportOptions};
use roomkit::{
    AttachChannelOptions, ConversationStore, CreateRoomOptions, ProcessInboundOptions, RoomKit,
    RoomKitOptions,
};
use roomkit_providers_sqlite::{SqliteStore, SqliteStoreOptions};

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

/// What the person writes, which the second kit reads back.
const MESSAGE: &str = "Hello, kept in SQLite.";

/// A store on the file `path`.
fn store(path: &Path) -> SqliteStore {
    SqliteStore::new(SqliteStoreOptions {
        path: path.to_path_buf(),
    })
}

/// The first kit: the room, its SMS channel, and the person's message.
async fn take_the_message(path: &Path, room: &RoomId) -> Outcome {
    let kit = RoomKit::new(RoomKitOptions::with_store(store(path)));
    let sms = ChannelId::new("sms")?;
    kit.register_channel(RecordingTransport::new(
        sms.clone(),
        ChannelType::SMS,
        RecordingTransportOptions::default(),
    ))?;
    kit.create_room(CreateRoomOptions {
        room_id: Some(room.clone()),
        ..CreateRoomOptions::default()
    })
    .await?;
    kit.attach_channel(room, &sms, AttachChannelOptions::default())
        .await?;
    let message = InboundMessage::new(sms, "+15551234567", TextContent::new(MESSAGE));
    let options = ProcessInboundOptions {
        room_id: Some(room.clone()),
        ..ProcessInboundOptions::default()
    };
    kit.process_inbound(message, options).await?;
    kit.close().await?;
    Ok(())
}

/// The texts of `room`'s timeline, read by a store of its own.
async fn texts(store: &SqliteStore, room: &RoomId) -> Outcome<Vec<String>> {
    let events = store
        .list_events(room, ListEventsOptions::default())
        .await?;
    Ok(events
        .into_iter()
        .filter_map(|event| {
            if let EventContent::Text(text) = event.content {
                Some(text.body)
            } else {
                None
            }
        })
        .collect())
}

/// The example's file, in the system's temporary directory, with the
/// write-ahead log's files beside it.
fn files() -> (PathBuf, [PathBuf; 3]) {
    let path =
        std::env::temp_dir().join(format!("roomkit-room-on-sqlite-{}.db", std::process::id()));
    let wal = path.with_extension("db-wal");
    let shm = path.with_extension("db-shm");
    (path.clone(), [path, wal, shm])
}

#[tokio::main]
async fn main() -> Outcome {
    let (path, written) = files();
    let room = RoomId::new("support")?;

    let read = async {
        take_the_message(&path, &room).await?;
        let reader = store(&path);
        let read = texts(&reader, &room).await?;
        reader.close().await;
        Ok::<_, Box<dyn Error>>(read)
    }
    .await;
    for file in &written {
        if file.exists() {
            std::fs::remove_file(file)?;
        }
    }
    let read = read?;
    println!("the second kit read {read:?} from {}", path.display());
    if read != [MESSAGE] {
        return Err(format!("the room should hold {MESSAGE:?}, read {read:?}").into());
    }
    Ok(())
}
