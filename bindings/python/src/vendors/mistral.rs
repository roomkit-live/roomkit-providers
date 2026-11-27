//! Mistral's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_mistral::{MistralConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Mistral's models, on its chat completions.
#[derive(Debug)]
pub(crate) struct Mistral;

impl Vendor for Mistral {
    type Config = MistralConfig;
    type Provider = roomkit_providers_mistral::Mistral;

    const NAME: &'static str = "mistral";
    const RECORDED_AS: &'static str = "MistralAIProvider";
    const LABEL: &'static str = "mistral";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: MistralConfig, key: Option<String>) -> MistralConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: MistralConfig) -> Result<Self::Provider, ProviderError> {
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
