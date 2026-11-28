//! `OpenAI`'s image part: its configuration, how its provider is built, and
//! the names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::ReplayWaits;
use roomkit_providers_openai::OpenAiImageConfig;

use crate::image_vendor::ImageVendor;

/// `OpenAI`'s image models.
#[derive(Debug)]
pub(crate) struct OpenAiImage;

impl ImageVendor for OpenAiImage {
    type Config = OpenAiImageConfig;
    type Provider = roomkit_providers_openai::OpenAiImage;

    const NAME: &'static str = "openai";
    const RECORDED_AS: &'static str = "OpenAIImageProvider";
    const LABEL: &'static str = "openai";

    fn keyed(mut config: OpenAiImageConfig, key: Option<String>) -> OpenAiImageConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: OpenAiImageConfig) -> Result<Self::Provider, ProviderError> {
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
