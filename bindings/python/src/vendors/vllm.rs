//! vLLM's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{ReplayWaits, VllmConfig};

use crate::vendor::Vendor;

/// A vLLM server's Chat Completions.
#[derive(Debug)]
pub(crate) struct Vllm;

impl Vendor for Vllm {
    type Config = VllmConfig;
    type Provider = roomkit_providers_openai::Vllm;

    const NAME: &'static str = "vllm";
    const RECORDED_AS: &'static str = "vllm";
    const LABEL: &'static str = "vllm";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: VllmConfig, key: Option<String>) -> VllmConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: VllmConfig) -> Result<Self::Provider, ProviderError> {
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
