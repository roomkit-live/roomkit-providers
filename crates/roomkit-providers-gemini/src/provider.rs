//! The Gemini provider: Python's `GeminiAIProvider`, on the Developer API.
//!
//! Every generation is asked as a stream, a whole answer too
//! (`streamGenerateContent?alt=sse`), with the key in `x-goog-api-key`. A
//! request is sent once: the google-genai SDK RoomKit's provider runs on
//! tries no request again. The listing reads every page the endpoint
//! gives, and fills what it leaves out from the catalogue.

use std::fmt;
use std::sync::Arc;

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use roomkit_ai::{
    AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing,
};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::stream::{leaving, text, timed, whole};
use roomkit_providers_client::{Call, Client, Exit, Settings, Timeouts, tried_again};
use serde_json::Value;

use crate::config::GeminiConfig;
use crate::endpoint::{DEVELOPER_API, KEY_HEADER};
use crate::failure::{self, Failure, past_mark};
use crate::models;
use crate::request::Asked;
use crate::stream;

/// The provider its failures name.
const LABEL: &str = "gemini";

/// The provider's name, as Python's class names itself.
const CLASS_NAME: &str = "GeminiAIProvider";

/// The most pages of the listing read: a bound on a server that would page
/// for ever.
const PAGES_AT_MOST: usize = 100;

/// No retry is taken (the SDK takes none): the rule only bounds what a
/// server may ask.
const RETRY: Rule = Rule {
    longest_asked: 60.0,
    past_it: Past::Longest,
};

/// A Gemini model of the Developer API (RFC §6.7): Python's
/// `GeminiAIProvider(config)`. A turn's reasoning goes as Gemini's thinking
/// config, a level or a budget as the model takes it; a round's calls go
/// back with their thought signatures, a round another vendor answered as
/// text; a tool's schema as Gemini declares it.
#[derive(Clone)]
pub struct Gemini {
    pub(crate) config: Arc<GeminiConfig>,
    pub(crate) generate: reqwest::Url,
    pub(crate) models: reqwest::Url,
    pub(crate) headers: HeaderMap,
    pub(crate) client: Client,
    pub(crate) token_source: Option<Arc<dyn crate::TokenSource>>,
    exit: Arc<Exit>,
}

impl Gemini {
    /// The provider `config` configures.
    ///
    /// # Errors
    ///
    /// A failure before any request: a key that cannot go in a header, a
    /// model that cannot go in a URL, a timeout the clock cannot count.
    pub fn new(config: GeminiConfig) -> Result<Self, ProviderError> {
        Self::built(config).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    /// Authenticate each Developer API request with a fresh bearer token. The
    /// quota project identifies the consumer of the API for Cloud billing.
    ///
    /// # Errors
    ///
    /// An API key in the configuration, an invalid quota project, or any
    /// configuration error that [`Self::new`] returns.
    pub fn with_oauth(
        config: GeminiConfig,
        quota_project: &str,
        source: Arc<dyn crate::TokenSource>,
    ) -> Result<Self, ProviderError> {
        if !config.api_key.is_empty() {
            return Err(Exit::unbuilt(
                LABEL,
                &Failure::unbuilt("OAuth and an API key cannot be combined"),
            ));
        }
        if quota_project.is_empty()
            || quota_project.len() > 63
            || !quota_project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(Exit::unbuilt(
                LABEL,
                &Failure::unbuilt("invalid Google Cloud quota project"),
            ));
        }
        let project = HeaderValue::from_str(quota_project).map_err(|_| {
            Exit::unbuilt(
                LABEL,
                &Failure::unbuilt("invalid Google Cloud quota project"),
            )
        })?;
        let mut provider = Self::new(config)?;
        provider.headers.remove(KEY_HEADER);
        provider.headers.insert("x-goog-user-project", project);
        provider.token_source = Some(source);
        Ok(provider)
    }

    fn built(config: GeminiConfig) -> Result<Self, Failure> {
        let path = format!("/models/{}:streamGenerateContent", config.model);
        let mut generate = joined(DEVELOPER_API, &path)?;
        generate.query_pairs_mut().append_pair("alt", "sse");
        let models = joined(DEVELOPER_API, "/models")?;
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let key = HeaderValue::from_str(&config.api_key);
        let mut key = key.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        key.set_sensitive(true);
        headers.insert(HeaderName::from_static(KEY_HEADER), key);
        let client = Client::new(Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: 0,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::gemini"),
        })?;
        let exit = client.exit(LABEL, [config.api_key.clone()], [], &generate);
        Ok(Self {
            config: Arc::new(config),
            generate,
            models,
            headers,
            token_source: None,
            client,
            exit: Arc::new(exit),
        })
    }

    /// The name the provider's failures carry: Python's `provider`.
    #[must_use]
    pub fn label(&self) -> &'static str {
        LABEL
    }

    pub(crate) fn asked(&self) -> Asked<'_> {
        Asked {
            config: &self.config,
            provider: LABEL,
        }
    }

    /// Sends the requests to `address` instead, over plain HTTP: a test's
    /// fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let urls = [&mut self.generate, &mut self.models];
        let moved = self.client.pointed_at(urls, address);
        assert!(moved, "the endpoint is pointed at the fake");
    }

    /// The provider a replay of recorded cases drives: its requests sent to
    /// `fake`, a loopback address, over plain HTTP, a date read against
    /// `clock`.
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
    ) -> Result<(Self, ReplayWaits), ProviderError> {
        let urls = [&mut self.generate, &mut self.models];
        let moved = roomkit_providers_client::replayed_at(urls, fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// Every page of the listing, each asked with the token of the one
    /// before.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let mut live = Vec::new();
        let mut token: Option<String> = None;
        for _ in 0..PAGES_AT_MOST {
            let mut url = self.models.clone();
            if let Some(token) = &token {
                url.query_pairs_mut().append_pair("pageToken", token);
            }
            let call = Call {
                url: &url,
                headers: &self.headers,
                body: None,
                token: self.token_source.as_deref(),
            };
            let (page, json) = self.client.send(call).await?;
            let page: Value = match serde_json::from_slice(past_mark(&page)) {
                Ok(page) => page,
                Err(error) if json => return Err(Failure::malformed(&error)),
                Err(_) => return Err(failure::no_listing()),
            };
            live.extend(models::listed(&page).ok_or_else(failure::no_listing)?);
            let next = page.get("nextPageToken").and_then(Value::as_str);
            match next.filter(|next| !next.is_empty()) {
                Some(next) => token = Some(next.to_owned()),
                None => break,
            }
        }
        Ok(live)
    }
}

/// A refusal the done event carries: Gemini's refusal stop reason, or why
/// the prompt itself was blocked.
fn refusal(done: &StreamDone) -> Option<String> {
    let refused = done.metadata.get("refusal").and_then(Value::as_str);
    refused.map(str::to_owned)
}

impl AiProvider for Gemini {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Every Gemini model reads images.
    fn supports_vision(&self) -> bool {
        true
    }

    fn available_models() -> Vec<ModelInfo> {
        models::models().to_vec()
    }

    fn supports_response_schema(&self) -> bool {
        true
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        true
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let live = self.live_models().await;
        let live = live.map_err(|failure| self.exit.leave(&failure))?;
        Ok(listing(live, models::models()))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, request: &AiContext) -> Result<AiResponse, ProviderError> {
        whole(self.generate_structured_stream(request)).await
    }

    fn generate_stream<'a>(&'a self, request: &'a AiContext) -> TextStream<'a> {
        text(self.generate_structured_stream(request))
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiContext) -> ProviderStream<'a> {
        let events = timed(
            stream::events(self, request),
            self.name(),
            &self.config.model,
        );
        let checked = checked_stream(events, request, LABEL, refusal);
        leaving(checked, &self.exit, request)
    }
}

#[cfg(feature = "replay")]
#[doc(hidden)]
pub use roomkit_providers_client::ReplayWaits;

impl fmt::Debug for Gemini {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gemini")
            .field("config", &self.config)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}
