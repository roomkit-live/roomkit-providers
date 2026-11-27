//! PolarGrid's part: its configuration, how its provider is built, and the
//! names `RoomKit` knows it by.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_polargrid::{PolarGridConfig, ReplayWaits};

use crate::vendor::Vendor;

/// The models of a PolarGrid edge, on its chat completions.
#[derive(Debug)]
pub(crate) struct PolarGrid;

impl Vendor for PolarGrid {
    type Config = PolarGridConfig;
    type Provider = roomkit_providers_polargrid::PolarGrid;

    const NAME: &'static str = "polargrid";
    const RECORDED_AS: &'static str = "PolarGridAIProvider";
    const LABEL: &'static str = "polargrid";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    // The configuration always states a key: an empty one sends the key
    // POLARGRID_API_KEY holds, as polargrid-sdk reads it.
    fn keyed(mut config: PolarGridConfig, key: Option<String>) -> PolarGridConfig {
        config.api_key = key.unwrap_or_default();
        config
    }

    fn built(config: PolarGridConfig) -> Result<Self::Provider, ProviderError> {
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
