//! Kimi's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{KimiConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Kimi's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct Kimi;

impl Vendor for Kimi {
    type Config = KimiConfig;
    type Provider = roomkit_providers_openai::Kimi;

    const NAME: &'static str = "kimi";
    const RECORDED_AS: &'static str = "kimi";
    const LABEL: &'static str = "kimi";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: KimiConfig, key: Option<String>) -> KimiConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: KimiConfig) -> Result<Self::Provider, ProviderError> {
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
