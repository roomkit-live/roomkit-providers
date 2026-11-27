//! MiniMax's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{MiniMaxConfig, ReplayWaits};

use crate::vendor::Vendor;

/// MiniMax's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct MiniMax;

impl Vendor for MiniMax {
    type Config = MiniMaxConfig;
    type Provider = roomkit_providers_openai::MiniMax;

    const NAME: &'static str = "minimax";
    const RECORDED_AS: &'static str = "minimax";
    const LABEL: &'static str = "minimax";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: MiniMaxConfig, key: Option<String>) -> MiniMaxConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: MiniMaxConfig) -> Result<Self::Provider, ProviderError> {
        Self::Provider::new(config)
    }

    fn replaying(
        provider: Self::Provider,
        fake: SocketAddr,
        clock: f64,
    ) -> Result<(Self::Provider, ReplayWaits), ProviderError> {
        provider.replaying(fake, clock)
    }
}
