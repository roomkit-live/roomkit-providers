//! OpenRouter's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{OpenRouterConfig, ReplayWaits};

use crate::vendor::Vendor;

/// OpenRouter's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct OpenRouter;

impl Vendor for OpenRouter {
    type Config = OpenRouterConfig;
    type Provider = roomkit_providers_openai::OpenRouter;

    const NAME: &'static str = "openrouter";
    const RECORDED_AS: &'static str = "OpenRouterAIProvider";
    const LABEL: &'static str = "openrouter";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: OpenRouterConfig, key: Option<String>) -> OpenRouterConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: OpenRouterConfig) -> Result<Self::Provider, ProviderError> {
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
