//! The Mistral provider: Python's `MistralAIProvider`, on Mistral's chat
//! completions.
//!
//! Every generation is asked as a stream, a whole answer too, with the key
//! as a bearer token. A request is sent once: the mistralai SDK RoomKit's
//! provider runs on tries no request again, and waits up to 300 seconds for
//! each step of one. The listing reads the one page the endpoint gives, and
//! fills what it leaves out from the catalogue.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use roomkit_ai::{
    AiContext, AiProvider, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing,
};
use roomkit_providers_client::endpoint::{is_vendor_endpoint, joined};
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::stream::{leaving, text, timed, whole};
use roomkit_providers_client::{Call, Client, Exit, Settings, Timeouts, tried_again};
use serde_json::Value;

use crate::config::MistralConfig;
use crate::endpoint::{CHAT_PATH, MISTRAL_BASE_URL, MODELS_PATH};
use crate::failure::{self, Failure, past_mark};
use crate::models;
use crate::request::Asked;
use crate::stream;

/// The provider its failures name.
const LABEL: &str = "mistral";

/// The provider's name, as Python's class names itself.
const CLASS_NAME: &str = "MistralAIProvider";

/// How long a request may wait to connect, and for each read and write:
/// the SDK's client's, which RoomKit's provider does not set.
const SDK_TIMEOUT: Duration = Duration::from_secs(300);

/// No retry is taken (the SDK takes none): the rule only bounds what a
/// server may ask.
const RETRY: Rule = Rule {
    longest_asked: 60.0,
    past_it: Past::Longest,
};

/// A Mistral model (RFC §6.7): Python's `MistralAIProvider(config)`. A
/// turn's reasoning goes as Mistral's `reasoning_effort`, read back from
/// `<think>` tags or from typed parts of reasoning; a tool's name is
/// checked on Mistral's own endpoint, and a response schema goes strict.
#[derive(Clone)]
pub struct Mistral {
    pub(crate) config: Arc<MistralConfig>,
    pub(crate) chat: reqwest::Url,
    pub(crate) models: reqwest::Url,
    pub(crate) chat_headers: HeaderMap,
    listing_headers: HeaderMap,
    own_endpoint: bool,
    pub(crate) client: Client,
    exit: Arc<Exit>,
}

impl Mistral {
    /// The provider `config` configures.
    ///
    /// # Errors
    ///
    /// A failure before any request: a key that cannot go in a header, a
    /// server URL or a model that cannot go in a URL.
    pub fn new(config: MistralConfig) -> Result<Self, ProviderError> {
        Self::built(config).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    fn built(config: MistralConfig) -> Result<Self, Failure> {
        let base = config.server_url.as_deref().unwrap_or(MISTRAL_BASE_URL);
        let chat = joined(base, CHAT_PATH)?;
        let models = joined(base, MODELS_PATH)?;
        let own_endpoint = is_vendor_endpoint(config.server_url.as_deref(), &[MISTRAL_BASE_URL]);
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| Failure::unbuilt("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        let mut listing_headers = HeaderMap::new();
        listing_headers.insert(AUTHORIZATION, bearer);
        listing_headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let mut chat_headers = listing_headers.clone();
        chat_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        chat_headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        let client = Client::new(Settings {
            timeouts: Timeouts::new(SDK_TIMEOUT, SDK_TIMEOUT)?,
            max_retries: 0,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::mistral"),
        })?;
        let exit = client.exit(LABEL, [config.api_key.clone()], [], &chat);
        Ok(Self {
            config: Arc::new(config),
            chat,
            models,
            chat_headers,
            listing_headers,
            own_endpoint,
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
            own_endpoint: self.own_endpoint,
        }
    }

    /// Sends the requests to `address` instead, over plain HTTP: a test's
    /// fake of the endpoint.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let urls = [&mut self.chat, &mut self.models];
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
        let urls = [&mut self.chat, &mut self.models];
        let moved = roomkit_providers_client::replayed_at(urls, fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// The page of the listing, read as Python reads it.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let call = Call {
            url: &self.models,
            headers: &self.listing_headers,
            body: None,
            token: None,
        };
        let (page, json) = self.client.send(call).await?;
        let page: Value = match serde_json::from_slice(past_mark(&page)) {
            Ok(page) => page,
            Err(error) if json => return Err(Failure::malformed(&error)),
            Err(_) => return Err(failure::no_listing()),
        };
        models::listed(&page).ok_or_else(failure::no_listing)
    }
}

/// Mistral states no refusal of its own.
fn no_refusal(_done: &StreamDone) -> Option<String> {
    None
}

impl AiProvider for Mistral {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Images go through whatever the model: the API refuses a model that
    /// reads none.
    fn supports_vision(&self) -> bool {
        true
    }

    fn available_models() -> Vec<ModelInfo> {
        models::models().to_vec()
    }

    fn supports_response_schema(&self) -> bool {
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
        let checked = checked_stream(events, request, LABEL, no_refusal);
        leaving(checked, &self.exit, request)
    }
}

#[cfg(feature = "replay")]
#[doc(hidden)]
pub use roomkit_providers_client::ReplayWaits;

impl fmt::Debug for Mistral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mistral")
            .field("config", &self.config)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}
