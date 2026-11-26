//! The two ports on Redis keep the contract RoomKit's core states: the
//! checks are the core's own (`roomkit::testing`), which its in-memory
//! broker and registry pass too.
use std::sync::Arc;

use roomkit::testing::{check_turn_broker, check_turn_registry};

use super::support;

#[tokio::test]
async fn the_broker_on_redis_keeps_the_port() {
    let Some(url) = support::server("the broker's contract") else {
        return;
    };
    let node = support::node(&url, &support::prefix("broker-port"), false);
    check_turn_broker(Arc::new(node.broker())).await;
    node.close().await.unwrap();
}

#[tokio::test]
async fn the_registry_on_redis_keeps_the_port() {
    let Some(url) = support::server("the registry's contract") else {
        return;
    };
    let node = support::node(&url, &support::prefix("registry-port"), false);
    check_turn_registry(Arc::new(node.registry())).await;
    node.close().await.unwrap();
}
