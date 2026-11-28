//! Gemini's image part: its configuration, how its provider is built, and
//! the names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::ReplayWaits;
use roomkit_providers_gemini::GeminiImageConfig;

use crate::image_vendor::ImageVendor;

/// Gemini's image models, on the Interactions API.
#[derive(Debug)]
pub(crate) struct GeminiImage;

impl ImageVendor for GeminiImage {
    type Config = GeminiImageConfig;
    type Provider = roomkit_providers_gemini::GeminiImage;

    const NAME: &'static str = "gemini";
    const RECORDED_AS: &'static str = "GeminiImageProvider";
    const LABEL: &'static str = "gemini";

    fn keyed(mut config: GeminiImageConfig, key: Option<String>) -> GeminiImageConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: GeminiImageConfig) -> Result<Self::Provider, ProviderError> {
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
