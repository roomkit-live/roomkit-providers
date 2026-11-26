//! The backend on Redis: its messages and its registry kept by one Redis
//! server (Python's `RedisTurnBackend`).
use std::fmt;
use std::sync::Arc;

use redis::aio::ConnectionManager;
use roomkit::turns::{
    ChannelControl, ChannelResult, DistributedTurnBackend, DistributedTurnBackendOptions,
    ExecutorInfo, ExecutorsListener, RoomOwner, ServedExecutor, TurnBackend, TurnFuture, TurnLink,
    TurnRequest, TurnTimeouts,
};
use roomkit::{ConversationStore, RoomKitError, TurnExecutor};

use crate::services::{Connections, RedisTurnBroker, RedisTurnRegistry};

/// Keyword arguments of Python's `RedisTurnBackend(url, *, client, key_prefix, timeouts)`.
#[derive(Clone)]
pub struct RedisTurnBackendOptions {
    /// Reuse the application’s data connection manager, including its configured timeouts.
    /// The inbox and heartbeat open independent connections at the supplied URL.
    pub client: Option<ConnectionManager>,
    /// Test option matching Python: route this node's local turns through its Redis inbox.
    #[expect(
        clippy::pub_underscore_fields,
        reason = "the private Python test option has this exact name"
    )]
    pub _loop_back: bool,
    /// Namespace shared by the room owners and their workers.
    pub key_prefix: String,
    /// The time bounds of this backend's turns.
    pub timeouts: TurnTimeouts,
}

impl Default for RedisTurnBackendOptions {
    fn default() -> Self {
        Self {
            client: None,
            _loop_back: false,
            key_prefix: "roomkit:turns".into(),
            timeouts: TurnTimeouts::default(),
        }
    }
}

impl fmt::Debug for RedisTurnBackendOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTurnBackendOptions")
            .field("key_prefix", &self.key_prefix)
            .field("timeouts", &self.timeouts)
            .finish_non_exhaustive()
    }
}

/// One process's Redis turn backend. Clones share its node identity.
#[derive(Clone)]
pub struct RedisTurnBackend {
    node: DistributedTurnBackend,
    /// The Redis server its broker and its registry are kept by.
    redis: Arc<Connections>,
}

impl fmt::Debug for RedisTurnBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisTurnBackend")
            .field("node_id", &self.node.node_id())
            .field("key_prefix", &self.redis.prefix)
            .finish_non_exhaustive()
    }
}

impl RedisTurnBackend {
    /// Configure this process. The first backend operation opens its Redis connection.
    ///
    /// # Errors
    /// A supplied client requires a URL for the independent heartbeat connection.
    /// Invalid Redis URLs return a value error.
    pub fn new(url: Option<&str>, options: RedisTurnBackendOptions) -> Result<Self, RoomKitError> {
        if options.client.is_some() && url.is_none() {
            return Err(RoomKitError::Value(
                "RedisTurnBackend(client=...) needs url= too: its beat connects with it".into(),
            ));
        }
        let address = url
            .filter(|value| !value.is_empty())
            .unwrap_or("redis://localhost:6379");
        let client = redis::Client::open(address)
            .map_err(|_| RoomKitError::Value("invalid Redis turn backend URL".into()))?;
        let RedisTurnBackendOptions {
            client: data,
            _loop_back: loop_back,
            key_prefix,
            timeouts,
        } = options;
        let redis = Arc::new(Connections::new(client, data, key_prefix, timeouts));
        let node = DistributedTurnBackend::new(
            Arc::new(RedisTurnBroker(Arc::clone(&redis))),
            Arc::new(RedisTurnRegistry(Arc::clone(&redis))),
            DistributedTurnBackendOptions {
                timeouts,
                _loop_back: loop_back,
            },
        );
        Ok(Self { node, redis })
    }

    /// What carries this backend's messages: its broker on Redis, on the
    /// backend's connections.
    pub fn broker(&self) -> RedisTurnBroker {
        RedisTurnBroker(Arc::clone(&self.redis))
    }

    /// What this backend's processes share: its registry on Redis, on the
    /// backend's connections.
    pub fn registry(&self) -> RedisTurnRegistry {
        RedisTurnRegistry(Arc::clone(&self.redis))
    }

    /// This process's node identifier.
    pub fn node_id(&self) -> &str {
        self.node.node_id()
    }

    /// The time bounds given to the backend.
    pub fn timeouts(&self) -> TurnTimeouts {
        self.node.timeouts()
    }

    /// Locally served executors and other nodes' declarations. Only remote entries have a last heartbeat.
    ///
    /// # Errors
    /// Returns `backend_closed` once shutdown starts, or an error if the registry is unreachable.
    pub async fn executors(&self) -> Result<Vec<ExecutorInfo>, RoomKitError> {
        self.node.executors().await
    }

    /// Advertise and serve this executor. A later registration can replace a closed executor.
    ///
    /// # Errors
    /// A registration on a closed backend fails, as does publishing its first Redis heartbeat.
    pub async fn serve<S: ConversationStore>(
        &self,
        executor: TurnExecutor<S>,
    ) -> Result<(), RoomKitError> {
        self.node.serve(executor).await
    }

    /// Record a room's agent presence (§10.6.7). Exactly one owner claims each change.
    /// Passing `None` forgets the state and returns `false`, so the next attach publishes again.
    ///
    /// # Errors
    /// The backend is closed, or the Redis registry cannot be reached.
    pub async fn claim_presence(
        &self,
        channel_id: &str,
        room_id: &str,
        online: Option<bool>,
    ) -> Result<bool, RoomKitError> {
        self.node.claim_presence(channel_id, room_id, online).await
    }

    /// Withdraw this node and close the links it owns.
    ///
    /// # Errors
    /// Redis may be unavailable when withdrawing. Expired heartbeats are forgotten by readers.
    pub async fn close(&self) -> Result<(), RoomKitError> {
        self.node.close().await
    }
}

impl TurnBackend for RedisTurnBackend {
    fn submit(
        &self,
        request: TurnRequest,
    ) -> TurnFuture<'_, Result<Arc<dyn TurnLink>, RoomKitError>> {
        TurnBackend::submit(&self.node, request)
    }

    fn submit_to(
        &self,
        request: TurnRequest,
        executor: ExecutorInfo,
    ) -> TurnFuture<'_, Result<Arc<dyn TurnLink>, RoomKitError>> {
        TurnBackend::submit_to(&self.node, request, executor)
    }

    fn serve(&self, executor: Arc<dyn ServedExecutor>) -> TurnFuture<'_, Result<(), RoomKitError>> {
        TurnBackend::serve(&self.node, executor)
    }

    fn control(
        &self,
        message: ChannelControl,
    ) -> TurnFuture<'_, Result<Option<ChannelResult>, RoomKitError>> {
        TurnBackend::control(&self.node, message)
    }

    fn attend(&self, owner: Arc<dyn RoomOwner>) -> TurnFuture<'_, Result<(), RoomKitError>> {
        TurnBackend::attend(&self.node, owner)
    }

    fn claim_presence<'a>(
        &'a self,
        channel_id: &'a str,
        room_id: &'a str,
        online: Option<bool>,
    ) -> Option<TurnFuture<'a, Result<bool, RoomKitError>>> {
        TurnBackend::claim_presence(&self.node, channel_id, room_id, online)
    }

    fn watch_executors(&self, listener: ExecutorsListener) {
        TurnBackend::watch_executors(&self.node, listener);
    }

    fn executors(&self) -> Option<TurnFuture<'_, Result<Vec<ExecutorInfo>, RoomKitError>>> {
        TurnBackend::executors(&self.node)
    }

    fn executors_now(&self) -> Option<TurnFuture<'_, Result<Vec<ExecutorInfo>, RoomKitError>>> {
        TurnBackend::executors_now(&self.node)
    }

    fn timeouts(&self) -> Option<TurnTimeouts> {
        TurnBackend::timeouts(&self.node)
    }

    fn close(&self) -> TurnFuture<'_, Result<(), RoomKitError>> {
        TurnBackend::close(&self.node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redis::aio::ConnectionManagerConfig;

    /// The defaults of Python's `RedisTurnBackend(url, *, client, key_prefix,
    /// timeouts)`, and of its private `_loop_back`.
    #[test]
    fn the_options_default_as_pythons_keywords() {
        let RedisTurnBackendOptions {
            client,
            _loop_back: loop_back,
            key_prefix,
            timeouts,
        } = RedisTurnBackendOptions::default();
        assert!(client.is_none());
        assert_eq!(key_prefix, "roomkit:turns");
        assert!(!loop_back);
        assert_eq!(timeouts, TurnTimeouts::default());
    }

    #[tokio::test]
    async fn only_an_absent_url_is_refused_with_an_injected_client() {
        let default = redis::ConnectionAddr::Tcp("localhost".into(), 6379);
        let custom = redis::Client::open("redis://example.test:6380").unwrap();
        let injected = RedisTurnBackendOptions {
            client: Some(
                custom
                    .get_connection_manager_lazy(ConnectionManagerConfig::new())
                    .unwrap(),
            ),
            ..Default::default()
        };
        let refused = RedisTurnBackend::new(None, injected.clone()).unwrap_err();
        assert!(
            matches!(refused, RoomKitError::Value(message) if message == "RedisTurnBackend(client=...) needs url= too: its beat connects with it")
        );
        for url in [None, Some("")] {
            let backend = RedisTurnBackend::new(url, RedisTurnBackendOptions::default()).unwrap();
            assert_eq!(backend.redis.address.get_connection_info().addr(), &default);
            assert!(backend.redis.data.get().is_none());
        }
        let backend = RedisTurnBackend::new(Some(""), injected).unwrap();
        assert_eq!(backend.redis.address.get_connection_info().addr(), &default);
        assert!(backend.redis.data.get().is_some());
    }
}
