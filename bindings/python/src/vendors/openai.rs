//! `OpenAI`'s part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_providers_llm::ProviderError;
use roomkit_providers_openai::{OpenAiConfig, ReplayWaits};

use crate::vendor::Vendor;

/// `OpenAI`'s Chat Completions.
#[derive(Debug)]
pub(crate) struct OpenAi;

impl Vendor for OpenAi {
    type Config = OpenAiConfig;
    type Provider = roomkit_providers_openai::OpenAi;

    const NAME: &'static str = "openai";
    const RECORDED_AS: &'static str = "OpenAIAIProvider";
    const LABEL: &'static str = "openai";

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: OpenAiConfig, key: Option<String>) -> OpenAiConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: OpenAiConfig) -> Result<Self::Provider, ProviderError> {
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
