//! The managed llama.cpp provider in the Python package.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::ReplayWaits;
use roomkit_providers_llamacpp::LlamaCppConfig;

use crate::vendor::Vendor;

/// Managed local inference.
#[derive(Debug)]
pub(crate) struct LlamaCpp;

impl Vendor for LlamaCpp {
    type Config = LlamaCppConfig;
    type Provider = roomkit_providers_llamacpp::LlamaCpp;

    const NAME: &'static str = "llamacpp";
    const RECORDED_AS: &'static str = "llamacpp";
    const LABEL: &'static str = "llamacpp";

    fn label(provider: &Self::Provider) -> &str {
        provider.label()
    }

    fn managed(provider: &Self::Provider) -> Option<std::sync::Arc<dyn crate::provider::Managed>> {
        Some(std::sync::Arc::new(provider.clone()))
    }

    fn keyed(config: LlamaCppConfig, _key: Option<String>) -> LlamaCppConfig {
        config
    }

    fn built(config: LlamaCppConfig) -> Result<Self::Provider, ProviderError> {
        Self::Provider::new(config)
    }

    fn in_host(
        config: LlamaCppConfig,
        bundle: impl FnOnce() -> Option<std::path::PathBuf>,
    ) -> Result<Self::Provider, ProviderError> {
        Self::Provider::from_host(config, bundle())
    }

    fn replaying(
        provider: Self::Provider,
        fake: SocketAddr,
        clock: f64,
    ) -> Result<(Self::Provider, ReplayWaits), ProviderError> {
        provider.replaying(fake, clock)
    }
}

impl crate::provider::Managed for roomkit_providers_llamacpp::LlamaCpp {
    fn base_url(&self) -> String {
        self.base_url()
    }

    fn start(
        &self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ProviderError>> + Send + '_>> {
        Box::pin(self.start())
    }
}
