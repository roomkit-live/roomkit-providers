//! xAI's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_openai::{ReplayWaits, XaiConfig};

use crate::vendor::Vendor;

/// xAI's Chat Completions, on its own endpoint or behind a base URL.
#[derive(Debug)]
pub(crate) struct Xai;

impl Vendor for Xai {
    type Config = XaiConfig;
    type Provider = roomkit_providers_openai::Xai;

    const NAME: &'static str = "xai";
    const RECORDED_AS: &'static str = "XAIAIProvider";
    const LABEL: &'static str = "xai";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key, an empty one for a server
    // that asks for none: with none stated, nothing is sent in its place.
    fn keyed(mut config: XaiConfig, key: Option<String>) -> XaiConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: XaiConfig) -> Result<Self::Provider, ProviderError> {
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
