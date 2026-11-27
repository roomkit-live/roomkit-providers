//! Cerebras's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{CerebrasConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Cerebras's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct Cerebras;

impl Vendor for Cerebras {
    type Config = CerebrasConfig;
    type Provider = roomkit_providers_openai::Cerebras;

    const NAME: &'static str = "cerebras";
    const RECORDED_AS: &'static str = "cerebras";
    const LABEL: &'static str = "cerebras";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: CerebrasConfig, key: Option<String>) -> CerebrasConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: CerebrasConfig) -> Result<Self::Provider, ProviderError> {
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
