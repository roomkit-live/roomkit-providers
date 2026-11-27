//! Qwen's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{QwenConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Qwen's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct Qwen;

impl Vendor for Qwen {
    type Config = QwenConfig;
    type Provider = roomkit_providers_openai::Qwen;

    const NAME: &'static str = "qwen";
    const RECORDED_AS: &'static str = "QwenAIProvider";
    const LABEL: &'static str = "qwen";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: QwenConfig, key: Option<String>) -> QwenConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: QwenConfig) -> Result<Self::Provider, ProviderError> {
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
