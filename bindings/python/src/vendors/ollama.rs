//! Ollama's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_ollama::{OllamaConfig, ReplayWaits};

use crate::vendor::Vendor;

/// The models an Ollama server holds, on its native API.
#[derive(Debug)]
pub(crate) struct Ollama;

impl Vendor for Ollama {
    type Config = OllamaConfig;
    type Provider = roomkit_providers_ollama::Ollama;

    const NAME: &'static str = "ollama";
    const RECORDED_AS: &'static str = "OllamaAIProvider";
    const LABEL: &'static str = "ollama";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // A configuration may state no key: Ollama's local daemon asks for
    // none, and the SDK then sends the one OLLAMA_API_KEY holds, if any.
    fn keyed(mut config: OllamaConfig, key: Option<String>) -> OllamaConfig {
        config.api_key = key;
        config
    }

    fn built(config: OllamaConfig) -> Result<Self::Provider, ProviderError> {
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
