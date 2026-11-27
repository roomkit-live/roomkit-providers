//! Gemini on Vertex AI: Python's `GeminiVertexProvider`, the Gemini
//! provider of the Developer API on a Google Cloud project's region.
//!
//! What differs from [`Gemini`](crate::Gemini) is where a request goes and
//! who sends it: the host of the configuration's location (a region's,
//! `global`'s, or a multi-region's), the project and the location in the
//! path, as the google-genai SDK writes them; a token of the identity
//! google-auth would give (a service-account key, Application Default
//! Credentials, a borrowed account) in place of a key, with the project a
//! file bills; and the billing labels each request carries. The generation, the stream, the thinking and the
//! catalogue are the Developer API's.

mod config;
mod identity;
#[cfg(test)]
mod replay;

use std::fmt;
use std::sync::Arc;

use roomkit_ai::{
    AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderStream, TextStream,
};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::{Exit, Timeouts};

pub use self::config::GeminiVertexConfig;
use self::identity::Identity;
use crate::config::GeminiConfig;
use crate::failure::Failure;
use crate::provider::{Gemini, LABEL, Parts};

/// The provider's name, as Python's class names itself.
const CLASS_NAME: &str = "GeminiVertexProvider";

/// The API version the SDK asks Vertex AI at.
const VERSION: &str = "v1beta1";

/// The multi-regions, asked on a host of their own.
const MULTI_REGIONS: [&str; 2] = ["us", "eu"];

/// A Gemini model on Vertex AI, in a Google Cloud project's region (RFC
/// §6.7): Python's `GeminiVertexProvider(config)`. The Developer API's
/// generation, stream, thinking and catalogue, called as a service account.
#[derive(Clone)]
pub struct GeminiVertex {
    inner: Gemini,
    config: Arc<GeminiVertexConfig>,
    /// Where Application Default Credentials are read: the process's, or a
    /// test's.
    #[cfg(any(test, feature = "replay"))]
    environment: roomkit_auth::Environment,
    /// The clock and the waits a test or a replay sets, which the identity
    /// keeps to as the client does.
    #[cfg(any(test, feature = "replay"))]
    pace: Arc<std::sync::Mutex<roomkit_providers_client::Pace>>,
}

/// The host the SDK asks `location` on.
fn host(location: &str) -> String {
    if location == "global" {
        "https://aiplatform.googleapis.com".to_owned()
    } else if MULTI_REGIONS.contains(&location) {
        format!("https://aiplatform.{location}.rep.googleapis.com")
    } else {
        format!("https://{location}-aiplatform.googleapis.com")
    }
}

/// The resource of `model`, as the SDK names a model on Vertex AI
/// (`t_model`): a name it already gives whole, a publisher's model, or
/// Google's.
fn resource(model: &str) -> Result<String, Failure> {
    if model.is_empty() || model.contains("..") || model.contains(['?', '&']) {
        return Err(Failure::unbuilt("its model cannot go in a URL"));
    }
    let whole = ["projects/", "models/", "publishers/"];
    if whole.iter().any(|start| model.starts_with(start)) {
        return Ok(model.to_owned());
    }
    Ok(match model.split_once('/') {
        Some((publisher, model)) => format!("publishers/{publisher}/models/{model}"),
        None => format!("publishers/google/models/{model}"),
    })
}

impl GeminiVertex {
    /// The provider `config` configures, as the configuration's
    /// service-account key, else Application Default Credentials (the
    /// file `GOOGLE_APPLICATION_CREDENTIALS` names, gcloud's file, the
    /// metadata server), a borrowed service account borrowed with it. A
    /// key and a file are read now, from the process's environment;
    /// nothing is asked before the first call. A file that cannot be read
    /// fails the first call, as Python's provider fails then.
    ///
    /// # Errors
    ///
    /// A failure before any request: labels Google refuses, a key that is
    /// no service-account key, a model or a location that cannot go in a
    /// URL, a timeout the clock cannot count.
    pub fn new(config: GeminiVertexConfig) -> Result<Self, ProviderError> {
        let environment = roomkit_auth::Environment::of_the_process();
        Self::built(config, &environment).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    fn built(
        config: GeminiVertexConfig,
        environment: &roomkit_auth::Environment,
    ) -> Result<Self, Failure> {
        config
            .checked_labels()
            .map_err(|_| Failure::unbuilt("its labels are not Google's"))?;
        let cloud = identity::of(&config, environment)?;
        let quota_project = cloud.quota_project().map(str::to_owned);
        let host = host(&config.location);
        let model = resource(&config.model)?;
        let path = if model.starts_with("projects/") {
            format!("/{VERSION}/{model}:streamGenerateContent")
        } else {
            let (project, location) = (&config.project, &config.location);
            format!(
                "/{VERSION}/projects/{project}/locations/{location}/{model}:streamGenerateContent"
            )
        };
        let mut generate = joined(&host, &path)?;
        generate.query_pairs_mut().append_pair("alt", "sse");
        let models = joined(&host, &format!("/{VERSION}/publishers/google/models"))?;
        let gemini = GeminiConfig {
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            thinking_level: config.thinking_level.clone(),
            timeout: config.timeout,
            connect_timeout: config.connect_timeout,
            ..GeminiConfig::new("")
        };
        let labels = config.labels.clone().filter(|labels| !labels.is_empty());
        let inner = Gemini::of(Parts {
            config: gemini,
            class_name: CLASS_NAME,
            generate,
            models,
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            token_source: Arc::new(Identity(cloud)),
            labels,
            quota_project,
        })?;
        Ok(Self {
            inner,
            config: Arc::new(config),
            #[cfg(any(test, feature = "replay"))]
            environment: environment.clone(),
            #[cfg(any(test, feature = "replay"))]
            pace: Arc::default(),
        })
    }

    /// The name the provider's failures carry: Python's `provider`.
    #[must_use]
    pub fn label(&self) -> &'static str {
        LABEL
    }

    /// The identity, its token requests sent to `origin` on the clock and
    /// the waits of [`pace`](Self::pace).
    #[cfg(any(test, feature = "replay"))]
    fn identity_at(&mut self, origin: &str) -> Result<(), Failure> {
        let cloud = identity::of(&self.config, &self.environment)?;
        let (now, wait) = (Arc::clone(&self.pace), Arc::clone(&self.pace));
        let pace = roomkit_auth::Pace {
            now: Arc::new(move || {
                let pace = now.lock().map(|pace| pace.clone()).unwrap_or_default();
                (pace.now)()
            }),
            wait: Arc::new(move |duration| {
                let pace = wait.lock().map(|pace| pace.clone()).unwrap_or_default();
                (pace.wait)(duration)
            }),
        };
        let cloud = cloud
            .replaying(origin, pace)
            .map_err(|_| Failure::unbuilt("its identity cannot be sent to the fake"))?;
        self.inner.token_source = Some(Arc::new(Identity(cloud)));
        Ok(())
    }

    /// The provider `config` configures, Application Default Credentials
    /// read in `environment`: a test's.
    #[cfg(test)]
    pub(crate) fn new_in(
        config: GeminiVertexConfig,
        environment: &roomkit_auth::Environment,
    ) -> Result<Self, ProviderError> {
        Self::built(config, environment).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    /// Sends every request to `address` instead, over plain HTTP, the token
    /// requests of its identity included: a test's fake.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        self.inner.point_at(address);
        self.identity_at(&format!("http://{address}"))
            .expect("the identity is pointed at the fake");
    }

    /// The provider a replay of recorded cases drives: every request sent to
    /// `fake`, a loopback address, over plain HTTP, the token requests of
    /// its identity included; a date read against `clock`, and the waits of
    /// both kept.
    ///
    /// # Errors
    ///
    /// A `fake` that is no loopback address.
    #[cfg(feature = "replay")]
    #[doc(hidden)]
    pub fn replaying(
        mut self,
        fake: std::net::SocketAddr,
        clock: f64,
    ) -> Result<(Self, crate::ReplayWaits), ProviderError> {
        let (inner, waits) = self.inner.replaying(fake, clock)?;
        self.inner = inner;
        if let Ok(mut pace) = self.pace.lock() {
            *pace = self.inner.client.pace();
        }
        let exit = |failure: &Failure| Exit::unbuilt(LABEL, failure);
        self.identity_at(&format!("http://{fake}"))
            .map_err(|failure| exit(&failure))?;
        Ok((self, waits))
    }
}

impl AiProvider for GeminiVertex {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Every Gemini model reads images.
    fn supports_vision(&self) -> bool {
        self.inner.supports_vision()
    }

    fn available_models() -> Vec<ModelInfo> {
        Gemini::available_models()
    }

    fn supports_response_schema(&self) -> bool {
        self.inner.supports_response_schema()
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        self.inner.supports_response_schema_with_tools()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.list_models().await
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, request: &AiContext) -> Result<AiResponse, ProviderError> {
        self.inner.generate(request).await
    }

    fn generate_stream<'a>(&'a self, request: &'a AiContext) -> TextStream<'a> {
        self.inner.generate_stream(request)
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        self.inner.generate_structured_stream(request)
    }
}

impl fmt::Debug for GeminiVertex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeminiVertex")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}
