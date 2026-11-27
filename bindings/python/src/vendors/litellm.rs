//! LiteLLM's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{LiteLlmConfig, ReplayWaits};

use crate::vendor::Vendor;

/// LiteLLM's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct LiteLlm;

impl Vendor for LiteLlm {
    type Config = LiteLlmConfig;
    type Provider = roomkit_providers_openai::LiteLlm;

    const NAME: &'static str = "litellm";
    const RECORDED_AS: &'static str = "LiteLLMAIProvider";
    const LABEL: &'static str = "litellm";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: LiteLlmConfig, key: Option<String>) -> LiteLlmConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: LiteLlmConfig) -> Result<Self::Provider, ProviderError> {
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
