//! Azure's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{AzureAiConfig, ReplayWaits};

use crate::vendor::Vendor;

/// An Azure resource's deployments, OpenAI's Chat Completions at their own
/// paths.
#[derive(Debug)]
pub(crate) struct AzureAi;

impl Vendor for AzureAi {
    type Config = AzureAiConfig;
    type Provider = roomkit_providers_openai::AzureAi;

    const NAME: &'static str = "azure";
    const RECORDED_AS: &'static str = "AzureAIProvider";
    const LABEL: &'static str = "azure";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: AzureAiConfig, key: Option<String>) -> AzureAiConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: AzureAiConfig) -> Result<Self::Provider, ProviderError> {
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
