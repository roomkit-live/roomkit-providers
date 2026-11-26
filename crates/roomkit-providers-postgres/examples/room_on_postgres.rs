//! A room's message kept in Postgres, read back by a second kit.
//!
//! A first kit, on a Postgres store, takes a person's message into a room
//! and closes. A second kit, on a new store over the same database, reads
//! the room's timeline: the message is there, as the first kit stored it.
//! The example checks it, removes its room, and fails otherwise.
//!
//! It needs a Postgres server, which `POSTGRES_DSN` names:
//!
//! ```sh
//! docker run --rm -e POSTGRES_PASSWORD=example -p 127.0.0.1:5432:5432 postgres:16
//! POSTGRES_DSN=postgresql://postgres:example@127.0.0.1:5432/postgres \
//!     cargo run -p roomkit-providers-postgres --example room_on_postgres
//! ```
#![expect(clippy::print_stdout, reason = "an example prints what it ran")]

use std::env;
use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use roomkit::model::{ChannelId, ChannelType, EventContent, InboundMessage, RoomId, TextContent};
use roomkit::store::ListEventsOptions;
use roomkit::testing::{RecordingTransport, RecordingTransportOptions};
use roomkit::{
    AttachChannelOptions, ConversationStore, CreateRoomOptions, ProcessInboundOptions, RoomKit,
    RoomKitOptions,
};
use roomkit_providers_postgres::{InitOptions, PostgresStore, PostgresStoreOptions};

type Outcome<T = ()> = Result<T, Box<dyn Error>>;

/// What the person writes, which the second kit reads back.
const MESSAGE: &str = "Hello, kept in Postgres.";

/// A store on the database `dsn` names, its schema in place.
async fn store(dsn: &str) -> Outcome<PostgresStore> {
    let store = PostgresStore::new(PostgresStoreOptions {
        dsn: Some(dsn.to_owned()),
        ..PostgresStoreOptions::default()
    });
    store.init(InitOptions::default()).await?;
    Ok(store)
}

/// The first kit: the room, its SMS channel, and the person's message.
async fn take_the_message(dsn: &str, room: &RoomId) -> Outcome {
    let kit = RoomKit::new(RoomKitOptions::with_store(store(dsn).await?));
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
async fn texts(store: &PostgresStore, room: &RoomId) -> Outcome<Vec<String>> {
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

#[tokio::main]
async fn main() -> Outcome {
    let dsn = env::var("POSTGRES_DSN").map_err(|_| "POSTGRES_DSN names no server")?;
    let started = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let room = RoomId::new(format!("example-{}-{started}", std::process::id()))?;

    take_the_message(&dsn, &room).await?;

    let reader = store(&dsn).await?;
    let read = texts(&reader, &room).await;
    reader.delete_room(&room).await?;
    let read = read?;
    println!("the second kit read {read:?} in room {room}");
    if read != [MESSAGE] {
        return Err(format!("the room should hold {MESSAGE:?}, read {read:?}").into());
    }
    Ok(())
}
