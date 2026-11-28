//! OpenRouter's image part: its configuration, how its provider is built, and
//! the names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::ReplayWaits;
use roomkit_providers_openai::OpenRouterImageConfig;

use crate::image_vendor::ImageVendor;

/// The image models OpenRouter routes to, on its Image API.
#[derive(Debug)]
pub(crate) struct OpenRouterImage;

impl ImageVendor for OpenRouterImage {
    type Config = OpenRouterImageConfig;
    type Provider = roomkit_providers_openai::OpenRouterImage;

    const NAME: &'static str = "openrouter";
    const RECORDED_AS: &'static str = "OpenRouterImageProvider";
    const LABEL: &'static str = "openrouter";

    fn keyed(mut config: OpenRouterImageConfig, key: Option<String>) -> OpenRouterImageConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: OpenRouterImageConfig) -> Result<Self::Provider, ProviderError> {
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
