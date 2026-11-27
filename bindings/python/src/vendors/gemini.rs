//! Gemini's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_gemini::{GeminiConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Gemini's models, on the Developer API.
#[derive(Debug)]
pub(crate) struct Gemini;

impl Vendor for Gemini {
    type Config = GeminiConfig;
    type Provider = roomkit_providers_gemini::Gemini;

    const NAME: &'static str = "gemini";
    const RECORDED_AS: &'static str = "GeminiAIProvider";
    const LABEL: &'static str = "gemini";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: GeminiConfig, key: Option<String>) -> GeminiConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: GeminiConfig) -> Result<Self::Provider, ProviderError> {
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
