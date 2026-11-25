//! What the functional tests share: the Redis server `REDIS_URL` names, and
//! the processes of one deployment on it, under a key prefix no other test
//! has.
use std::time::Duration;

use roomkit::TurnTimeouts;
use roomkit_providers_redis::{RedisTurnBackend, RedisTurnBackendOptions};

/// The test server's URL. Without `REDIS_URL` the test says it is skipped
/// and passes: the default checks need no service.
#[expect(clippy::print_stderr, reason = "a skipped test says so")]
pub(super) fn server(test: &str) -> Option<String> {
    let url = std::env::var("REDIS_URL").ok();
    if url.is_none() {
        eprintln!("{test}: skipped, REDIS_URL names no Redis server");
    }
    url
}

/// A key prefix of this test's own.
pub(super) fn prefix(label: &str) -> String {
    format!("roomkit:test:{label}:{}", uuid::Uuid::new_v4())
}

/// The bounds of a test's backends: a beat every 50 ms.
pub(super) fn quick() -> TurnTimeouts {
    TurnTimeouts {
        heartbeat_interval: Duration::from_millis(50),
        ..TurnTimeouts::default()
    }
}

/// A process of the deployment `prefix` names on the server at `url`.
pub(super) fn node(url: &str, prefix: &str, loop_back: bool) -> RedisTurnBackend {
    RedisTurnBackend::new(
        Some(url),
        RedisTurnBackendOptions {
            key_prefix: prefix.to_owned(),
            timeouts: quick(),
            _loop_back: loop_back,
            ..RedisTurnBackendOptions::default()
        },
    )
    .unwrap()
}

/// A connection of the test's own to the server, to read what it keeps.
pub(super) async fn connection(url: &str) -> redis::aio::MultiplexedConnection {
    redis::Client::open(url)
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap()
}
