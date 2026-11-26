//! The backend's services on Redis: a stream per node's inbox, the beats
//! and the registry in sorted sets and hashes written by scripts, at Redis
//! server time in milliseconds.
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig, MultiplexedConnection};
use redis::streams::{StreamId, StreamReadReply};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use roomkit::RoomKitError;
use roomkit::turns::{
    Beat, BeatWriter as Beater, Beats, BrokerMessage as Message, InboxEntry as Entry,
    RegistryView as View, TurnBroker, TurnFuture, TurnInbox as Inbox, TurnRegistry, TurnTimeouts,
};

/// The Redis server a backend's services are kept by: its address, the
/// data connection its commands share, the keys' prefix and the bounds
/// its connections and its keys' lifetimes are taken from.
pub(crate) struct Connections {
    pub(crate) prefix: String,
    timeouts: TurnTimeouts,
    pub(crate) address: redis::Client,
    pub(crate) data: OnceCell<ConnectionManager>,
}

/// The messages of a distributed turn backend, carried by a Redis server:
/// a stream per node's inbox (`<prefix>:inbox:<node>`). Clones share its
/// connections.
#[derive(Clone)]
pub struct RedisTurnBroker(pub(crate) Arc<Connections>);

/// What the processes of a distributed turn backend share, kept by a Redis
/// server: their beats, each room's holder and the presence last
/// published. Clones share its connections.
#[derive(Clone)]
pub struct RedisTurnRegistry(pub(crate) Arc<Connections>);

impl fmt::Debug for RedisTurnBroker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTurnBroker")
            .field("key_prefix", &self.0.prefix)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for RedisTurnRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTurnRegistry")
            .field("key_prefix", &self.0.prefix)
            .finish_non_exhaustive()
    }
}

impl Connections {
    pub(crate) fn new(
        address: redis::Client,
        client: Option<ConnectionManager>,
        prefix: String,
        timeouts: TurnTimeouts,
    ) -> Self {
        Self {
            prefix,
            timeouts,
            address,
            data: OnceCell::new_with(client),
        }
    }

    pub(crate) async fn connection(&self) -> Result<ConnectionManager, RoomKitError> {
        let bound = (self.timeouts.heartbeat_timeout / 2).max(Duration::from_millis(1));
        self.data
            .get_or_try_init(|| async {
                let config = ConnectionManagerConfig::new()
                    .set_connection_timeout(Some(bound))
                    .set_response_timeout(Some(bound))
                    .set_number_of_retries(0);
                ConnectionManager::new_with_config(self.address.clone(), config)
                    .await
                    .map_err(|error| redis_error(&error))
            })
            .await
            .cloned()
    }

    pub(crate) fn key(&self, suffix: &str) -> String {
        format!("{}:{suffix}", self.prefix)
    }

    pub(crate) fn ttl_ms(&self) -> u64 {
        u64::try_from(
            self.timeouts
                .heartbeat_timeout
                .as_millis()
                .saturating_mul(3),
        )
        .unwrap_or(u64::MAX)
        .max(1)
    }

    pub(crate) async fn inbox_connection(
        &self,
        block: Duration,
    ) -> Result<MultiplexedConnection, RoomKitError> {
        let bound = (self.timeouts.heartbeat_timeout / 2).max(Duration::from_millis(1));
        let config = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(Some(bound))
            .set_response_timeout(Some(bound + block));
        self.address
            .get_multiplexed_async_connection_with_config(&config)
            .await
            .map_err(|error| redis_error(&error))
    }

    pub(crate) async fn beat_connection(&self) -> Result<MultiplexedConnection, RoomKitError> {
        let bound = (self.timeouts.heartbeat_timeout / 2).max(Duration::from_millis(1));
        let cfg = redis::AsyncConnectionConfig::new()
            .set_connection_timeout(Some(bound))
            .set_response_timeout(Some(bound));
        self.address
            .get_multiplexed_async_connection_with_config(&cfg)
            .await
            .map_err(|error| redis_error(&error))
    }

    fn holder_key(&self, channel: &str, room: &str) -> String {
        self.key(&format!("holder:{}:{}", quoted(channel), quoted(room)))
    }
}

async fn write_beat(
    prefix: &str,
    ttl_ms: u64,
    connection: &mut impl redis::aio::ConnectionLike,
    beat: &Beat,
) -> Result<(), RoomKitError> {
    let key = |suffix: &str| format!("{prefix}:{suffix}");
    let suffix = |name: &str| key(&format!("{name}:{}", beat.node));
    let payload = serde_json::to_string(beat).map_err(|e| RoomKitError::Runtime(e.to_string()))?;
    redis::cmd("EVAL")
        .arg(include_str!("beat.lua"))
        .arg(6)
        .arg(key("alive"))
        .arg(key("executors"))
        .arg(key("owners"))
        .arg(suffix("inbox"))
        .arg(suffix("running"))
        .arg(suffix("generation"))
        .arg(ttl_ms)
        .arg(payload)
        .exec_async(connection)
        .await
        .map_err(|error| redis_error(&error))
}

impl TurnBroker for RedisTurnBroker {
    fn post<'a>(
        &'a self,
        node: &'a str,
        message: &'a Message,
    ) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let key = self.0.key(&format!("inbox:{node}"));
            let mut connection = self.0.connection().await?;
            // A crash between adding a message and setting the TTL would leave an orphaned inbox.
            redis::pipe()
                .atomic()
                .cmd("XADD")
                .arg(&key)
                .arg("*")
                .arg("kind")
                .arg(&message.kind)
                .arg("sender")
                .arg(&message.sender)
                .arg("body")
                .arg(&message.body)
                .arg("target")
                .arg(&message.target)
                .arg("ref")
                .arg(&message.reference)
                .ignore()
                .cmd("PEXPIRE")
                .arg(key)
                .arg(self.0.ttl_ms())
                .ignore()
                .query_async(&mut connection)
                .await
                .map_err(|error| redis_error(&error))
        })
    }

    fn inbox<'a>(
        &'a self,
        node: &'a str,
        block: Duration,
    ) -> TurnFuture<'a, Result<Box<dyn Inbox>, RoomKitError>> {
        Box::pin(async move {
            let inbox: Box<dyn Inbox> = Box::new(RedisInbox {
                connection: self.0.inbox_connection(block).await?,
                key: self.0.key(&format!("inbox:{node}")),
                block,
            });
            Ok(inbox)
        })
    }

    fn acknowledge<'a>(
        &'a self,
        node: &'a str,
        ids: &'a [String],
    ) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let mut connection = self.0.connection().await?;
            let key = self.0.key(&format!("inbox:{node}"));
            redis::cmd("XDEL")
                .arg(key)
                .arg(ids)
                .exec_async(&mut connection)
                .await
                .map_err(|error| redis_error(&error))
        })
    }
}

impl TurnRegistry for RedisTurnRegistry {
    fn connect(&self) -> TurnFuture<'_, Result<(), RoomKitError>> {
        Box::pin(async move { self.0.connection().await.map(drop) })
    }

    fn connected(&self) -> bool {
        self.0.data.get().is_some()
    }

    fn beat<'a>(&'a self, beat: &'a Beat) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let mut connection = self.0.connection().await?;
            write_beat(&self.0.prefix, self.0.ttl_ms(), &mut connection, beat).await
        })
    }

    fn beater(&self) -> TurnFuture<'_, Result<Box<dyn Beater>, RoomKitError>> {
        Box::pin(async move {
            let beater: Box<dyn Beater> = Box::new(RedisBeater {
                connection: self.0.beat_connection().await?,
                prefix: self.0.prefix.clone(),
                ttl_ms: self.0.ttl_ms(),
            });
            Ok(beater)
        })
    }

    fn view(&self, forget_after_ms: i64) -> TurnFuture<'_, Result<View, RoomKitError>> {
        Box::pin(async move {
            let mut connection = self.0.connection().await?;
            let body: String = redis::cmd("EVAL")
                .arg(include_str!("view.lua"))
                .arg(3)
                .arg(self.0.key("alive"))
                .arg(self.0.key("executors"))
                .arg(self.0.key("owners"))
                .arg(forget_after_ms)
                .query_async(&mut connection)
                .await
                .map_err(|error| redis_error(&error))?;
            let view: ScriptView =
                serde_json::from_str(&body).map_err(|e| RoomKitError::Runtime(e.to_string()))?;
            Ok(View {
                now: view.now,
                executors: executors(view.executors),
                owners: serde_json::from_value(view.owners).unwrap_or_default(),
            })
        })
    }

    fn forget_owner<'a>(&'a self, node: &'a str) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let mut connection = self.0.connection().await?;
            redis::cmd("ZREM")
                .arg(self.0.key("owners"))
                .arg(node)
                .exec_async(&mut connection)
                .await
                .map_err(|error| redis_error(&error))
        })
    }

    fn beats<'a>(
        &'a self,
        members: &'a [&'a str],
        running: &'a [(&'a str, Vec<&'a str>)],
    ) -> TurnFuture<'a, Result<Beats, RoomKitError>> {
        Box::pin(async move {
            // Redis refuses either command with nothing to look up: what
            // names none is asked nothing, and answered nothing.
            let mut pipe = redis::pipe();
            pipe.atomic();
            if !members.is_empty() {
                pipe.cmd("ZMSCORE").arg(self.0.key("alive")).arg(members);
            }
            let asked = running.iter().filter(|(_, ids)| !ids.is_empty());
            for (node, ids) in asked.clone() {
                pipe.cmd("SMISMEMBER")
                    .arg(self.0.key(&format!("running:{node}")))
                    .arg(ids);
            }
            let rows: Vec<Vec<Option<i64>>> = if members.is_empty() && asked.count() == 0 {
                Vec::new()
            } else {
                let mut client = self.0.connection().await?;
                pipe.query_async(&mut client)
                    .await
                    .map_err(|error| redis_error(&error))?
            };
            let mut rows = rows.into_iter();
            let mut row = |named: bool| named.then(|| rows.next()).flatten().unwrap_or_default();
            Ok(Beats {
                last: row(!members.is_empty()),
                running: running
                    .iter()
                    .map(|(_, ids)| {
                        let runs = row(!ids.is_empty()).into_iter();
                        runs.map(|runs| runs == Some(1)).collect()
                    })
                    .collect(),
            })
        })
    }

    fn holder<'a>(
        &'a self,
        channel: &'a str,
        room: &'a str,
    ) -> TurnFuture<'a, Result<Option<String>, RoomKitError>> {
        Box::pin(async move {
            let mut conn = self.0.connection().await?;
            redis::cmd("GET")
                .arg(self.0.holder_key(channel, room))
                .query_async(&mut conn)
                .await
                .map_err(|error| redis_error(&error))
        })
    }

    fn hold<'a>(
        &'a self,
        channel: &'a str,
        room: &'a str,
        member: &'a str,
    ) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let mut conn = self.0.connection().await?;
            redis::cmd("SET")
                .arg(self.0.holder_key(channel, room))
                .arg(member)
                .arg("EX")
                .arg(7 * 24 * 3600)
                .exec_async(&mut conn)
                .await
                .map_err(|error| redis_error(&error))
        })
    }

    fn forget_holder<'a>(
        &'a self,
        channel: &'a str,
        room: &'a str,
    ) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(async move {
            let mut conn = self.0.connection().await?;
            redis::cmd("DEL")
                .arg(self.0.holder_key(channel, room))
                .exec_async(&mut conn)
                .await
                .map_err(|error| redis_error(&error))
        })
    }

    fn claim_presence<'a>(
        &'a self,
        channel_id: &'a str,
        room_id: &'a str,
        online: Option<bool>,
    ) -> TurnFuture<'a, Result<bool, RoomKitError>> {
        Box::pin(async move {
            let key = self.0.key(&format!(
                "presence:{}:{}",
                quoted(room_id),
                quoted(channel_id)
            ));
            let state = online.map_or("", |value| if value { "online" } else { "offline" });
            let mut connection = self.0.connection().await?;
            redis::cmd("EVAL")
                .arg(include_str!("claim_presence.lua"))
                .arg(1)
                .arg(key)
                .arg(state)
                .arg(7 * 24 * 60 * 60 * 1000)
                .query_async(&mut connection)
                .await
                .map_err(|error| redis_error(&error))
        })
    }
}

struct RedisInbox {
    connection: MultiplexedConnection,
    key: String,
    block: Duration,
}

impl Inbox for RedisInbox {
    fn read<'a>(&'a mut self, last: &'a str) -> TurnFuture<'a, Result<Vec<Entry>, RoomKitError>> {
        Box::pin(async move {
            let block = u64::try_from(self.block.as_millis()).unwrap_or(1000);
            let last = if last.is_empty() { "0-0" } else { last };
            let batch: Option<StreamReadReply> = redis::cmd("XREAD")
                .arg("COUNT")
                .arg(256)
                .arg("BLOCK")
                .arg(block)
                .arg("STREAMS")
                .arg(&self.key)
                .arg(last)
                .query_async(&mut self.connection)
                .await
                .map_err(|error| redis_error(&error))?;
            Ok(batch
                .into_iter()
                .flat_map(|r| r.keys)
                .flat_map(|k| k.ids)
                .map(|entry| Entry {
                    message: message(&entry),
                    id: entry.id,
                })
                .collect())
        })
    }
}

/// What `view.lua` answers.
#[derive(Deserialize)]
struct ScriptView {
    now: i64,
    owners: Value,
    executors: Value,
}

/// Each executor's last beat and declaration, none when the script's table
/// does not read as one: Redis Lua encodes an empty table as either [] or {}.
fn executors(table: Value) -> BTreeMap<String, (i64, Option<String>)> {
    let Ok(entries) = serde_json::from_value::<BTreeMap<String, (i64, Value)>>(table) else {
        return BTreeMap::new();
    };
    entries
        .into_iter()
        .map(|(member, (beat, declared))| (member, (beat, declared.as_str().map(str::to_owned))))
        .collect()
}

pub(crate) fn message(entry: &StreamId) -> Option<Message> {
    Some(Message {
        kind: entry.get("kind")?,
        sender: entry.get("sender")?,
        body: entry.get("body")?,
        target: entry.get("target").unwrap_or_default(),
        reference: entry.get("ref").unwrap_or_default(),
    })
}

struct RedisBeater {
    connection: MultiplexedConnection,
    prefix: String,
    ttl_ms: u64,
}

impl Beater for RedisBeater {
    fn beat<'a>(&'a mut self, beat: &'a Beat) -> TurnFuture<'a, Result<(), RoomKitError>> {
        Box::pin(write_beat(
            &self.prefix,
            self.ttl_ms,
            &mut self.connection,
            beat,
        ))
    }
}

/// Python's urllib quote(safe="") applied to one key component.
fn quoted(input: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn redis_error(error: &redis::RedisError) -> RoomKitError {
    RoomKitError::Runtime(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RedisTurnBackend, RedisTurnBackendOptions};

    async fn connection_id(connection: &mut impl redis::aio::ConnectionLike) -> i64 {
        redis::cmd("CLIENT")
            .arg("ID")
            .query_async(connection)
            .await
            .unwrap()
    }

    /// The data connection belongs to the application; the inbox and the
    /// beat use links of their own. Runs on the server `REDIS_URL` names.
    #[tokio::test]
    async fn an_injected_pool_keeps_its_response_timeout_ownership_and_connection() {
        let Ok(url) = std::env::var("REDIS_URL") else {
            eprintln!("an injected pool: skipped, REDIS_URL names no Redis server");
            return;
        };
        let client = redis::Client::open(url.as_str()).unwrap();
        let config =
            ConnectionManagerConfig::new().set_response_timeout(Some(Duration::from_secs(2)));
        let mut application = ConnectionManager::new_with_config(client, config)
            .await
            .unwrap();
        let options = RedisTurnBackendOptions {
            client: Some(application.clone()),
            key_prefix: format!("roomkit:client-proof:{}", uuid::Uuid::new_v4()),
            timeouts: TurnTimeouts {
                heartbeat_timeout: Duration::from_millis(100),
                ..Default::default()
            },
            ..Default::default()
        };
        let backend = RedisTurnBackend::new(Some(&url), options).unwrap();
        let redis = backend.broker().0;
        let mut data = redis.connection().await.unwrap();
        let mut blocking = redis::cmd("BLPOP");
        blocking
            .arg(format!("roomkit:client-proof:{}", uuid::Uuid::new_v4()))
            .arg(0.25_f64);
        let read = blocking.query_async::<Option<(String, String)>>(&mut data);
        {
            tokio::pin!(read);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), &mut read)
                    .await
                    .is_err()
            );
            assert_eq!(read.await.unwrap(), None);
        }
        let app_id = connection_id(&mut application).await;
        assert_eq!(connection_id(&mut data).await, app_id);
        let block = Duration::from_millis(50);
        let inbox_id = connection_id(&mut redis.inbox_connection(block).await.unwrap()).await;
        let beat_id = connection_id(&mut redis.beat_connection().await.unwrap()).await;
        assert_ne!(inbox_id, beat_id);
        assert_ne!(inbox_id, app_id);
        assert_ne!(beat_id, app_id);
        assert!(
            backend
                .claim_presence("agent", "room", Some(true))
                .await
                .unwrap()
        );
        assert!(!backend.claim_presence("agent", "room", None).await.unwrap());
        backend.close().await.unwrap();
        drop(backend);
        assert_eq!(connection_id(&mut application).await, app_id);
    }
}
