//! What Redis keeps of a backend: the keys RoomKit's Python reads and
//! writes, and how long each lasts.
use roomkit::turns::{BrokerMessage, TurnBroker, TurnRegistry};

use super::support;

const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

#[tokio::test]
async fn a_presence_claim_is_kept_under_the_python_key_for_a_week() {
    let Some(url) = support::server("a presence claim's key") else {
        return;
    };
    let prefix = support::prefix("presence-key");
    let node = support::node(&url, &prefix, false);
    let (agent, room) = ("a:b", "r/É");
    assert!(node.claim_presence(agent, room, Some(true)).await.unwrap());
    assert!(node.claim_presence(agent, room, Some(false)).await.unwrap());

    // Python's quote(safe="") of each part: the room, then the channel.
    let key = format!("{prefix}:presence:r%2F%C3%89:a%3Ab");
    let mut redis = support::connection(&url).await;
    let (state, ttl): (String, i64) = redis::pipe()
        .cmd("GET")
        .arg(&key)
        .cmd("PTTL")
        .arg(&key)
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(state, "offline");
    assert!((WEEK_MS - DAY_MS..=WEEK_MS).contains(&ttl), "{ttl}");
    node.close().await.unwrap();
}

#[tokio::test]
async fn a_rooms_holder_is_kept_under_the_python_key_until_forgotten() {
    let Some(url) = support::server("a room's holder key") else {
        return;
    };
    let prefix = support::prefix("holder-key");
    let node = support::node(&url, &prefix, false);
    let registry = node.registry();
    registry.hold("a:b", "r/É", "node:worker").await.unwrap();

    // Python's quote(safe="") of each part: the channel, then the room.
    let key = format!("{prefix}:holder:a%3Ab:r%2F%C3%89");
    let mut redis = support::connection(&url).await;
    let held: Option<String> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(held.as_deref(), Some("node:worker"));
    registry.forget_holder("a:b", "r/É").await.unwrap();
    let held: Option<String> = redis::cmd("GET")
        .arg(&key)
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!(held, None);
    node.close().await.unwrap();
}

#[tokio::test]
async fn an_inbox_is_a_stream_that_lasts_three_heartbeat_timeouts() {
    let Some(url) = support::server("an inbox's lifetime") else {
        return;
    };
    let prefix = support::prefix("inbox-key");
    let node = support::node(&url, &prefix, false);
    let message = BrokerMessage::new("frame", node.node_id(), "body".to_owned(), "turn", "");
    node.broker().post("peer", &message).await.unwrap();

    let key = format!("{prefix}:inbox:peer");
    let mut redis = support::connection(&url).await;
    let (kind, length, ttl): (String, i64, i64) = redis::pipe()
        .cmd("TYPE")
        .arg(&key)
        .cmd("XLEN")
        .arg(&key)
        .cmd("PTTL")
        .arg(&key)
        .query_async(&mut redis)
        .await
        .unwrap();
    assert_eq!((kind.as_str(), length), ("stream", 1));
    let bound = i64::try_from(node.timeouts().heartbeat_timeout.as_millis()).unwrap() * 3;
    assert!((1..=bound).contains(&ttl), "{ttl} of {bound}");
    node.close().await.unwrap();
}
