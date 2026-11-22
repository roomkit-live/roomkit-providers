//! `Anthropic`'s part: its configuration, how its provider is built, and
//! the names `RoomKit` knows it by.
//!
//! A base URL crosses as Python's configuration keeps it: without the path
//! of the Messages API it ended with. The Rust configuration takes a base
//! URL as a host writes it, and drops that path from it once more. So the
//! path is put back as the configuration is read ([`Kept`]): a base URL
//! that named the path twice keeps it once, on both sides.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_anthropic::{AnthropicConfig, ReplayWaits};
use serde::{Deserialize, Deserializer, Serialize};

use crate::vendor::Vendor;

/// The path the configuration of either side drops from the end of a base
/// URL.
const MESSAGES: &str = "/v1/messages";

/// The configuration as Python hands it, its base URL already kept.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(crate) struct Kept(AnthropicConfig);

impl<'de> Deserialize<'de> for Kept {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut config = AnthropicConfig::deserialize(deserializer)?;
        // What the Rust configuration keeps of this is what Python kept.
        // An empty base URL is kept as it is by both.
        config.base_url = config.base_url.map(|kept| {
            if kept.is_empty() {
                kept
            } else {
                format!("{kept}{MESSAGES}")
            }
        });
        Ok(Self(config))
    }
}

/// `Anthropic`'s Messages API.
#[derive(Debug)]
pub(crate) struct Anthropic;

impl Vendor for Anthropic {
    type Config = Kept;
    type Provider = roomkit_providers_anthropic::Anthropic;

    const NAME: &'static str = "anthropic";
    const RECORDED_AS: &'static str = "AnthropicAIProvider";
    const LABEL: &'static str = "anthropic";

    // The configuration always states a key: with none stated, nothing is
    // sent in its place.
    fn keyed(mut config: Kept, key: Option<String>) -> Kept {
        config.0.api_key = key.unwrap_or_default();
        config
    }

    fn built(Kept(config): Kept) -> Result<Self::Provider, ProviderError> {
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
