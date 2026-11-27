//! Gemini on Vertex AI's part: its configuration, how its provider is
//! built, and the names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_gemini::{GeminiVertexConfig, ReplayWaits};

use crate::vendor::Vendor;

/// Google's Gemini models on Vertex AI, in a project's region.
#[derive(Debug)]
pub(crate) struct GeminiVertex;

impl Vendor for GeminiVertex {
    type Config = GeminiVertexConfig;
    type Provider = roomkit_providers_gemini::GeminiVertex;

    const NAME: &'static str = "vertex";
    const RECORDED_AS: &'static str = "GeminiVertexProvider";
    const LABEL: &'static str = "gemini";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The key that crosses apart is the service-account key file's contents:
    // Vertex AI takes no API key.
    fn keyed(mut config: GeminiVertexConfig, key: Option<String>) -> GeminiVertexConfig {
        config.service_account_json = key;
        config
    }

    fn built(config: GeminiVertexConfig) -> Result<Self::Provider, ProviderError> {
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
