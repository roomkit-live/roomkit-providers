//! DeepSeek's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{DeepSeekConfig, ReplayWaits};

use crate::vendor::Vendor;

/// DeepSeek's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct DeepSeek;

impl Vendor for DeepSeek {
    type Config = DeepSeekConfig;
    type Provider = roomkit_providers_openai::DeepSeek;

    const NAME: &'static str = "deepseek";
    const RECORDED_AS: &'static str = "DeepSeekAIProvider";
    const LABEL: &'static str = "deepseek";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: DeepSeekConfig, key: Option<String>) -> DeepSeekConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: DeepSeekConfig) -> Result<Self::Provider, ProviderError> {
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
