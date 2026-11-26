//! A local model served by a managed llama.cpp process.

mod binary;
mod config;
mod diagnostics;
mod install;
mod loader;
mod server;

pub use config::LlamaCppConfig;

use futures_util::{TryStreamExt, stream};
use roomkit_ai::{
    AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderErrorOptions,
    ProviderStream, TextStream, schema_for_generate,
};
use roomkit_providers_openai::{OpenAi, OpenAiConfig};
use std::{
    fmt,
    net::{Ipv4Addr, TcpListener},
    sync::Arc,
};

const PROVIDER: &str = "llamacpp";

fn error(message: impl Into<String>) -> ProviderError {
    ProviderError::new(
        message,
        ProviderErrorOptions {
            provider: PROVIDER.into(),
            ..Default::default()
        },
    )
}

/// A managed llama.cpp server. Clones use the same process.
#[derive(Clone)]
pub struct LlamaCpp {
    wire: OpenAi,
    server: Arc<server::Server>,
    #[cfg(feature = "replay")]
    replay: bool,
}

impl LlamaCpp {
    /// Configure the server without installing a binary or loading a model.
    ///
    /// # Errors
    /// Invalid settings, no available local port, or an unavailable HTTP client.
    pub fn new(config: LlamaCppConfig) -> Result<Self, ProviderError> {
        Self::from_host(config, None)
    }

    /// Configure the child of a host whose packager redirected its shared-library paths.
    ///
    /// # Errors
    /// The same configuration and client failures as [`Self::new`].
    #[doc(hidden)]
    pub fn from_host(
        config: LlamaCppConfig,
        bundle: Option<std::path::PathBuf>,
    ) -> Result<Self, ProviderError> {
        config.validate()?;
        let port = match config.port {
            Some(port) => port,
            None => TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .and_then(|s| s.local_addr())
                .map_err(|e| error(format!("cannot allocate a loopback port: {e}")))?
                .port(),
        };
        // The reference local server client uses this sentinel on unauthenticated endpoints.
        let mut http = OpenAiConfig::new("none", &config.model);
        http.max_tokens = Some(config.max_tokens);
        http.timeout = config.timeout;
        let wire = OpenAi::for_llamacpp(http, port, config.enable_thinking)?;
        Ok(Self {
            wire,
            #[cfg(feature = "replay")]
            replay: false,
            server: Arc::new(server::Server::new(config, port, bundle)),
        })
    }

    /// The name carried by this provider's errors.
    pub fn label(&self) -> &'static str {
        PROVIDER
    }

    /// The local OpenAI-compatible endpoint.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.server.port)
    }

    /// Download missing resources and wait for the server to become ready.
    /// Concurrent calls share the same startup.
    ///
    /// # Errors
    /// The binary or model could not be loaded or the server did not become ready.
    pub async fn start(&self) -> Result<(), ProviderError> {
        #[cfg(feature = "replay")]
        if self.replay {
            return Ok(());
        }
        self.server.start().await
    }

    /// Route a wire fixture to its loopback fake without starting a child.
    ///
    /// # Errors
    /// The replay target is not on loopback.
    #[cfg(feature = "replay")]
    #[doc(hidden)]
    pub fn replaying(
        mut self,
        address: std::net::SocketAddr,
        clock: f64,
    ) -> Result<(Self, roomkit_providers_client::ReplayWaits), ProviderError> {
        let (wire, waits) = self.wire.replaying(address, clock)?;
        self.wire = wire;
        self.replay = true;
        Ok((self, waits))
    }

    async fn prepare(&self, request: &AiContext) -> Result<(), ProviderError> {
        schema_for_generate(
            request,
            self.supports_response_schema(),
            self.supports_response_schema_with_tools(),
            self.name(),
        )?;
        self.start().await
    }
}

impl fmt::Debug for LlamaCpp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlamaCpp")
            .field("port", &self.server.port)
            .finish_non_exhaustive()
    }
}

impl AiProvider for LlamaCpp {
    fn name(&self) -> &'static str {
        self.label()
    }
    fn model_name(&self) -> &str {
        self.wire.model_name()
    }
    fn supports_vision(&self) -> bool {
        true
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn supports_structured_streaming(&self) -> bool {
        true
    }
    fn supports_response_schema(&self) -> bool {
        self.wire.supports_response_schema()
    }
    fn supports_response_schema_with_tools(&self) -> bool {
        self.wire.supports_response_schema_with_tools()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.start().await?;
        self.wire.list_models().await
    }
    async fn generate(&self, request: &AiContext) -> Result<AiResponse, ProviderError> {
        self.prepare(request).await?;
        self.wire.generate(request).await
    }
    fn generate_stream<'a>(&'a self, request: &'a AiContext) -> TextStream<'a> {
        roomkit_providers_client::stream::text(self.generate_structured_stream(request))
    }
    fn generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        Box::pin(
            stream::once(async move {
                self.prepare(request).await?;
                Ok::<_, ProviderError>(self.wire.generate_structured_stream(request))
            })
            .try_flatten(),
        )
    }
    async fn close(&self) {
        self.server.close().await;
    }
}
