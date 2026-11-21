//! The provider: Anthropic's Messages API, and the servers that speak it
//! behind a base URL.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use reqwest::header::HeaderMap;
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::stream::{leaving, text};
use roomkit_providers_client::{Call, Client, Exit, Secrets, Settings, Timeouts, tried_again};
use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing, request_api_key,
};
use serde_json::Value;

use self::build::{Carried, KEY_HEADER, headers_of, key_value, own_endpoint, secrets_of};
use crate::config::AnthropicConfig;
use crate::endpoint::ANTHROPIC_BASE_URL;
use crate::failure::{self, Failure, past_mark};
use crate::models;
use crate::request::Asked;
use crate::stream;
use crate::whole;

mod build;

/// The provider's name in its failures: Python's `provider="anthropic"`.
const LABEL: &str = "anthropic";

/// What the provider is called: the name of Python's class, which
/// `AIProvider.name` answers.
const CLASS_NAME: &str = "AnthropicAIProvider";

/// The path of the Messages API under a base URL.
const MESSAGES: &str = "/v1/messages";

/// The path of the model listing under a base URL.
const MODELS: &str = "/v1/models";

/// The most models one page of the listing holds: the page is asked once.
const LISTED_AT_MOST: &str = "1000";

/// Where the retry rule of Python's anthropic SDK stands: a server may ask
/// for any wait, which is taken up to 4,294,967 seconds.
const RETRY: Rule = Rule {
    longest_asked: 4_294_967.0,
    past_it: Past::Longest,
};

/// What a request asks for.
const JSON: &str = "application/json";

/// An AI provider for Anthropic's Messages API, and for the servers that
/// speak it behind a base URL: Python's `AnthropicAIProvider`.
///
/// It answers a request whole ([`generate`](AiProvider::generate)) or as a
/// stream, of events
/// ([`generate_structured_stream`](AiProvider::generate_structured_stream))
/// or of text ([`generate_stream`](AiProvider::generate_stream)), and lists
/// the models its endpoint serves ([`list_models`](AiProvider::list_models)).
/// A request is sent to the base URL of the [configuration](AnthropicConfig),
/// Anthropic's own when it names none, with the configuration's key in its
/// `x-api-key` header, or the key the request carries for itself
/// ([`API_KEY_METADATA_KEY`](roomkit_providers_llm::API_KEY_METADATA_KEY)).
///
/// # Streams
///
/// Every generation is asked as a stream, a whole answer too, which is
/// folded from it. A stream is pulled: nothing is sent before it is first
/// polled, and the answer is read only as its events are taken. Dropping
/// the stream stops the request: its connection is closed, whatever was
/// left of the answer. A failure is the stream's last item; what was read
/// before it was handed on first. A request is tried again, as the
/// configuration allows, until the head of its answer came, never after: a
/// stream that fails under way fails, and the failure says whether a new
/// request may succeed.
///
/// # What it guarantees
///
/// - no failure's text repeats a secret of the configuration or of the
///   request that a server's answer holds, as it is or as the code that
///   builds the failure writes it again: the key, the key a request brings,
///   the value of an extra header, the user information of the base URL.
///   This holds whatever built the failure. A secret shorter than 8
///   characters is not searched for, and one a server itself sends back in
///   another form cannot be recognised; the `Debug` forms print none;
/// - a failure repeats of a server's answer only what an error object says
///   of itself (its message, its type), or a value a reader refuses, on one
///   line, without a control character or a character that shows nothing,
///   its message within 500 characters; a body that is no JSON is never
///   repeated;
/// - every size a server controls is bounded: a model listing past 4 MiB
///   fails the request; a stream fails at a line or an event past 1 MiB and
///   past 256 MiB in all, past 1,024 calls or 1,024 content blocks open at
///   once, and at a call's arguments past 4 MiB;
/// - a redirect is not followed, and no compressed answer is asked for; the
///   provider reads no key, no base URL and no header from the environment:
///   what its client reads of it, the proxies and the certificate
///   authorities it names, is `roomkit-providers-client`'s to say;
/// - the timeouts are the configuration's: `connect_timeout` to connect,
///   `timeout` for the head of an answer and between two reads of its body,
///   which also ends a stream that stalls.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, beside the key, as Python's client sends it. A base URL
/// for which Python's client and this one would not send the same request
/// is refused when the provider is built. The crate's README says each of
/// these in full.
#[derive(Clone)]
pub struct Anthropic {
    pub(crate) config: Arc<AnthropicConfig>,
    /// Where a message is asked.
    pub(crate) messages: reqwest::Url,
    /// Where the models are listed.
    pub(crate) models: reqwest::Url,
    headers: HeaderMap,
    /// Whether an extra header carries the key in place of any other.
    key_replaced: bool,
    /// Whether a request that brings no key still carries a credential.
    credential: bool,
    /// The client every request is sent with: its tries, its timeouts and
    /// the bounds of what it reads.
    pub(crate) client: Client,
    own_endpoint: bool,
    /// Where every failure leaves, cleaned of the configuration's secrets:
    /// the failure of a generation with its request, whose own key it is
    /// then cleaned of too.
    exit: Arc<Exit>,
}

impl Anthropic {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a base URL that is no `http` or
    /// `https` URL, or that has a query or a fragment, which the path of
    /// the API cannot follow; an extra header that is no HTTP header; or a
    /// client that could not be built.
    pub fn new(config: AnthropicConfig) -> Result<Self, ProviderError> {
        // A provider that cannot be built says why in its own words.
        let nothing_to_hide = Exit::new(LABEL, Secrets::default());
        Self::built(config).map_err(|failure| nothing_to_hide.leave(&failure))
    }

    fn built(config: AnthropicConfig) -> Result<Self, Failure> {
        let base_url = config.kept_base_url();
        let base_url = base_url.as_deref().unwrap_or(ANTHROPIC_BASE_URL);
        let messages = joined(base_url, MESSAGES)?;
        let mut models = joined(base_url, MODELS)?;
        models
            .query_pairs_mut()
            .append_pair("limit", LISTED_AT_MOST);
        let Carried {
            headers,
            key_replaced,
            credential,
        } = headers_of(&config, &messages)?;
        let client = Client::new(Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: config.max_retries,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::anthropic"),
        })?;
        let exit = Exit::new(LABEL, secrets_of(&config, &client, &messages));
        Ok(Self {
            own_endpoint: own_endpoint(&config, &messages),
            config: Arc::new(config),
            messages,
            models,
            headers,
            key_replaced,
            credential,
            client,
            exit: Arc::new(exit),
        })
    }

    /// The headers of the request that answers `request`: every request's,
    /// with the key `request` carries for itself in place of the
    /// configuration's, unless an extra header carries the key.
    ///
    /// # Errors
    ///
    /// The failure of a request that has no key to carry, and of a key
    /// that cannot go in a header.
    pub(crate) fn headers_of(&self, request: &AiRequest) -> Result<Cow<'_, HeaderMap>, Failure> {
        let lent = request_api_key(request).filter(|_| !self.key_replaced);
        let Some(key) = lent else {
            return self.keyed_headers();
        };
        let key = key_value(key).ok_or_else(failure::no_header_for_the_key)?;
        let mut headers = self.headers.clone();
        headers.insert(KEY_HEADER, key);
        Ok(Cow::Owned(headers))
    }

    /// The headers of a request that brings no key of its own.
    fn keyed_headers(&self) -> Result<Cow<'_, HeaderMap>, Failure> {
        if self.credential {
            Ok(Cow::Borrowed(&self.headers))
        } else {
            Err(failure::no_key())
        }
    }

    /// The URL the provider asks its messages at.
    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &reqwest::Url {
        &self.messages
    }

    /// Sends the requests to `address` instead, over plain HTTP, each to
    /// the path it had and with the user information it had: a test's fake
    /// of the endpoint. What the provider knows of its endpoint stays what
    /// its configuration says.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        let urls = [&mut self.messages, &mut self.models];
        let moved = self.client.pointed_at(urls, address);
        assert!(moved, "the endpoint is pointed at the fake");
    }

    /// The provider as a replay of recorded cases drives it, outside this
    /// crate's tests: its requests go to `fake`, a local fake of its
    /// endpoint, over plain HTTP, each to the path it had and with the user
    /// information it had; a `retry-after` date is read against `clock`, in
    /// seconds since 1970; and no wait is taken between two tries: each is
    /// kept, in the waits returned. What the provider knows of its endpoint
    /// stays what its configuration says.
    ///
    /// # Errors
    ///
    /// A `fake` that is no loopback address: a replay sends nothing off the
    /// machine.
    #[cfg(feature = "replay")]
    #[doc(hidden)]
    pub fn replaying(
        mut self,
        fake: std::net::SocketAddr,
        clock: f64,
    ) -> Result<(Self, ReplayWaits), ProviderError> {
        let urls = [&mut self.messages, &mut self.models];
        let moved = roomkit_providers_client::replayed_at(urls, fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// What a request is written for.
    pub(crate) fn asked(&self) -> Asked<'_> {
        Asked {
            config: &self.config,
            provider: LABEL,
            entry: models::entry(&self.config.model),
            own_endpoint: self.own_endpoint,
        }
    }

    /// The models the endpoint lists, each by its id and its name, as
    /// Python's `list_models` reads the one page it asks.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let headers = self.keyed_headers()?;
        let call = Call {
            url: &self.models,
            headers: &headers,
            body: None,
        };
        let (page, json) = self.client.send(call).await?;
        if !json {
            return Err(failure::no_listing());
        }
        let page: Value =
            serde_json::from_slice(past_mark(&page)).map_err(|error| Failure::malformed(&error))?;
        if failure::holds_an_error(&page) {
            return Err(Failure::error_object(&page));
        }
        models::listed(&page).ok_or_else(failure::no_listing)
    }
}

/// Why a constrained answer was withheld: the model refused (Python's
/// `_refusal`).
fn refusal(done: &StreamDone) -> Option<String> {
    (done.finish_reason.as_deref() == Some("refusal")).then(|| "refusal".to_owned())
}

impl AiProvider for Anthropic {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn label(&self) -> &'static str {
        LABEL
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Whether the configured model takes images, as Python's provider
    /// answers it: what the catalogue says of the model, whatever the
    /// endpoint; for a model it does not know, by how its name starts.
    fn supports_vision(&self) -> bool {
        models::sees_images(&self.config.model)
    }

    /// Anthropic's curated catalogue, Python's.
    fn available_models() -> Vec<ModelInfo> {
        models::models().to_vec()
    }

    fn supports_response_schema(&self) -> bool {
        true
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        true
    }

    /// Whether the model can hold a tool declared and unseen, as the
    /// catalogue says of it; never behind another base URL, where a proxy
    /// gets the request it always got.
    fn supports_deferred_tools(&self) -> bool {
        let entry = models::entry(&self.config.model);
        self.own_endpoint && models::tagged(entry, models::DEFERRED_TOOLS)
    }

    /// The models the endpoint lists, `GET /v1/models`, as Python's
    /// `list_models`: the page gives each model's id and name, what the
    /// catalogue knows of a model is filled from it, and a model it does
    /// not know passes through. Asked with the configuration's key.
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

    /// One whole answer, folded from
    /// [`generate_structured_stream`](AiProvider::generate_structured_stream):
    /// nothing of an answer that failed under way is given.
    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        whole::response(self.generate_structured_stream(request)).await
    }

    /// The text of
    /// [`generate_structured_stream`](AiProvider::generate_structured_stream),
    /// as it comes: its reasoning, its calls and its end are left out.
    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        text(self.generate_structured_stream(request))
    }

    /// The events of one generation, as they come: what the message's start
    /// tells of its input, the reasoning and the text, the composition of
    /// each call and the call itself, then the done event. An answer
    /// constrained by a response schema is checked before its done event,
    /// which its failure replaces. A failure leaves with `request`, whose
    /// own key it does not repeat.
    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        let checked = checked_stream(stream::events(self, request), request, LABEL, refusal);
        leaving(checked, &self.exit, request)
    }
}

/// The waits a replayed provider did not take between its tries (see
/// `Anthropic::replaying`).
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use roomkit_providers_client::ReplayWaits;

impl fmt::Debug for Anthropic {
    /// The configuration's `Debug` form: neither the key nor what a base
    /// URL holds beside its scheme, host and port.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Anthropic")
            .field("config", &self.config)
            .field("own_endpoint", &self.own_endpoint)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
