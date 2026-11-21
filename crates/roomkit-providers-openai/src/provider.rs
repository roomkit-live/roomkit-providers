//! The provider: `OpenAI`'s Chat Completions, and the servers that speak
//! it behind a base URL.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reqwest::header::HeaderMap;
use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing,
};

use roomkit_providers_http::sse::STREAM_LIMIT;

use crate::answer;
use crate::config::OpenAiConfig;
use crate::endpoint::{OPENAI_BASE_URL, is_openai_endpoint};

pub(crate) use self::build::Timeouts;
use self::build::{headers_of, joined, own_endpoint, refused, secrets_of};
use crate::exchange::{Call, Pace};
use crate::failure::Failure;
use crate::failure::exit::{Exit, Secrets};

mod build;
use crate::models;
use crate::request::Asked;
use crate::stream;

/// The provider's name in its failures: Python's `_provider_name`.
const NAME: &str = "openai";

/// What the provider is called: the name of Python's class, which
/// `AIProvider.name` answers.
const CLASS_NAME: &str = "OpenAIAIProvider";

/// The path of Chat Completions under a base URL.
const CHAT_COMPLETIONS: &str = "/chat/completions";

/// The path of the model listing under a base URL.
const MODELS: &str = "/models";

/// What a request asks for, and what the body of a completion's is.
pub(crate) const JSON: &str = "application/json";

/// An AI provider for `OpenAI`'s Chat Completions, and for the servers that
/// speak it behind a base URL: Python's `OpenAIAIProvider`.
///
/// It answers a request whole ([`generate`](AiProvider::generate)) or as a
/// stream, of events
/// ([`generate_structured_stream`](AiProvider::generate_structured_stream))
/// or of text ([`generate_stream`](AiProvider::generate_stream)), and lists
/// the models its endpoint serves ([`list_models`](AiProvider::list_models)).
/// A request is sent to the base URL of the [configuration](OpenAiConfig),
/// `OpenAI`'s own when it names none, with the configuration's key as its
/// bearer token.
///
/// # Streams
///
/// A stream is pulled: nothing is sent before it is first polled, and the
/// answer is read only as its events are taken. Dropping the stream stops
/// the request: its connection is closed, whatever was left of the answer.
/// A failure is the stream's last item; what was read before it was handed
/// on first. A request is tried again, as the configuration allows, until
/// the head of its answer came, never after: a stream that fails under way
/// fails, and the failure says whether a new request may succeed.
///
/// # What it guarantees
///
/// - no failure's text repeats a secret of the configuration that a
///   server's answer holds, as it is or as the code that builds the
///   failure writes it again: the key, the value of a default header, the
///   user information of the base URL. This holds whatever built the
///   failure. A secret shorter than 8 characters is not searched for, and
///   one a server itself sends back in another form cannot be recognised;
///   the `Debug` forms print none;
/// - a failure repeats of a server's answer only what an error object says
///   of itself (its message, its type, its code), or a value a reader
///   refuses, on one line, without a control character or a character that
///   shows nothing, its message within 500 characters; a body that is no
///   JSON is never repeated;
/// - every size a server controls is bounded: an answer read whole past
///   4 MiB fails the request; a stream fails at a line or an event past
///   1 MiB and past 256 MiB in all, past 1,024 calls, and at a call's
///   arguments or a refusal past 4 MiB;
/// - a redirect is not followed, and no proxy, key or URL is read from the
///   environment; no compressed answer is asked for;
/// - the timeouts are the configuration's: `connect_timeout` to connect,
///   `timeout` for the head of an answer and between two reads of its body,
///   which also ends a stream that stalls.
///
/// A base URL that holds user information is sent with it as Basic
/// credentials, in place of the key, as Python's client sends it. A base
/// URL for which Python's client and this one would not send the same
/// request is refused when the provider is built. The crate's README says
/// each of these in full.
#[derive(Clone)]
pub struct OpenAiAiProvider {
    pub(crate) config: Arc<OpenAiConfig>,
    /// Where a completion is asked.
    pub(crate) chat: reqwest::Url,
    /// Where the models are listed.
    pub(crate) models: reqwest::Url,
    pub(crate) headers: HeaderMap,
    pub(crate) client: reqwest::Client,
    own_endpoint: bool,
    left_out_warned: Arc<AtomicBool>,
    pub(crate) pace: Pace,
    /// The most bytes read of a streamed answer: the reader's bound, which
    /// a test lowers.
    pub(crate) stream_limit: usize,
    /// Where every failure leaves, cleaned of the configuration's secrets.
    exit: Arc<Exit>,
    /// The timeouts the client was built with.
    pub(crate) timeouts: Timeouts,
}

impl OpenAiAiProvider {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a base URL that is no `http` or
    /// `https` URL, or that has a query or a fragment, which the path of
    /// the API cannot follow; a default header that is no HTTP header; or a
    /// client that could not be built.
    pub fn new(config: OpenAiConfig) -> Result<Self, ProviderError> {
        // A provider that cannot be built says why in its own words.
        let nothing_to_hide = Exit {
            provider: NAME,
            secrets: Secrets::default(),
        };
        Self::built(config).map_err(|failure| nothing_to_hide.leave(&failure))
    }

    fn built(config: OpenAiConfig) -> Result<Self, Failure> {
        let base_url = config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
        let chat = joined(base_url, CHAT_COMPLETIONS)?;
        let models = joined(base_url, MODELS)?;
        let headers = headers_of(&config, &chat)?;
        let timeouts = Timeouts::of(&config)?;
        let tls = roomkit_providers_http::tls_config().map_err(|_| refused("TLS is not set"))?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            // No compressed answer is asked for, whatever a host's build
            // turns on in the HTTP client.
            .no_gzip()
            .no_brotli()
            .no_zstd()
            .no_deflate()
            // Neither the system's proxies nor its variables are read.
            .no_proxy()
            .read_timeout(timeouts.read)
            .connect_timeout(timeouts.connect)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| refused("the HTTP client could not be built"))?;
        let exit = Exit {
            provider: NAME,
            secrets: secrets_of(&config, &client, &chat),
        };
        Ok(Self {
            own_endpoint: own_endpoint(&config, &chat),
            config: Arc::new(config),
            chat,
            models,
            headers,
            client,
            left_out_warned: Arc::new(AtomicBool::new(false)),
            pace: Pace::default(),
            stream_limit: STREAM_LIMIT,
            exit: Arc::new(exit),
            timeouts,
        })
    }

    /// The URL the provider asks its completions at.
    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &reqwest::Url {
        &self.chat
    }

    /// Sends the requests to `address` instead, over plain HTTP, each to
    /// the path it had and with the user information it had: a test's fake
    /// of the endpoint. What the provider knows of its endpoint stays what
    /// its configuration says.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        assert!(
            self.moved_to(address),
            "the endpoint is pointed at the fake"
        );
    }

    /// Writes `address` as the host and the port of every URL the provider
    /// asks, over plain HTTP; `false` when a URL could not take it.
    #[cfg(any(test, feature = "replay"))]
    fn moved_to(&mut self, address: std::net::SocketAddr) -> bool {
        [&mut self.chat, &mut self.models].into_iter().all(|url| {
            url.set_scheme("http").is_ok()
                && url.set_ip_host(address.ip()).is_ok()
                && url.set_port(Some(address.port())).is_ok()
        })
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
        let no_loopback = refused("a replay sends to a loopback address only");
        let no_loopback = self.exit.leave(&no_loopback);
        if !fake.ip().is_loopback() || !self.moved_to(fake) {
            return Err(no_loopback);
        }
        let waits = ReplayWaits::default();
        let kept = waits.clone();
        self.pace = Pace {
            now: Arc::new(move || clock),
            wait: Arc::new(move |wait| {
                kept.keep(wait.as_secs_f64());
                Box::pin(std::future::ready(()))
            }),
        };
        Ok((self, waits))
    }

    /// What a request is written for.
    pub(crate) fn asked(&self) -> Asked<'_> {
        Asked {
            config: &self.config,
            provider: NAME,
            entry: models::entry(&self.config.model).filter(|_| self.own_endpoint),
            own_endpoint: self.own_endpoint,
            left_out_warned: &self.left_out_warned,
        }
    }

    /// One whole answer: the request, tried again as the configuration
    /// allows, and the response read from the answer.
    async fn answer(&self, request: &AiRequest) -> Result<AiResponse, Failure> {
        let body = self.asked().body(request, false)?;
        let body = serde_json::to_vec(&body).map_err(|_| unwritten())?;
        let (answer, json) = self.send(Call::Chat(&body)).await?;
        answer::read(&answer, json, request, &self.config.model, NAME)
    }

    /// The models the endpoint lists, each by its id alone, as Python's
    /// `list_models` reads the page.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let (page, json) = self.send(Call::Models).await?;
        let no_page =
            || Failure::unreadable("the model listing is no page of models with their ids");
        let listed = json.then(|| models::listed(&page)).flatten();
        listed.ok_or_else(no_page)
    }
}

/// Why a constrained answer was withheld: the refusal its deltas carried,
/// or a content filter's stop (Python's `_streamed_refusal`).
fn refusal(done: &StreamDone) -> Option<String> {
    let refused = done.metadata.get("refusal");
    let refused = refused.and_then(serde_json::Value::as_str);
    if let Some(refused) = refused.filter(|refused| !refused.is_empty()) {
        return Some(refused.to_owned());
    }
    (done.finish_reason.as_deref() == Some("content_filter")).then(|| "content_filter".to_owned())
}

/// The failure of a request that could not be written: final, and nothing
/// was sent.
pub(crate) fn unwritten() -> Failure {
    Failure::before_request("the request could not be written as JSON")
}

impl AiProvider for OpenAiAiProvider {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Whether the configured model takes images, as Python's provider
    /// answers it: what the catalogue says of the model, whatever the
    /// endpoint; for a model it does not know, yes behind a base URL, where
    /// the server decides what its model reads, and on `OpenAI`'s own
    /// endpoint by how the model's name starts. The configuration's
    /// `vision` states it where none of these does: `true` answers yes.
    fn supports_vision(&self) -> bool {
        if self.config.vision {
            return true;
        }
        let stated = models::entry(&self.config.model).and_then(|entry| entry.supports_vision);
        stated.unwrap_or_else(|| {
            !is_openai_endpoint(self.config.base_url.as_deref())
                || models::sees_images_by_its_name(&self.config.model)
        })
    }

    /// `OpenAI`'s curated catalogue, Python's.
    fn available_models() -> Vec<ModelInfo> {
        models::models().to_vec()
    }

    fn supports_response_schema(&self) -> bool {
        self.asked().supports_response_schema()
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        self.asked().supports_response_schema_with_tools()
    }

    /// Every model the endpoint lists, `GET /models`, as Python's
    /// `list_models`: the page carries ids alone, so what the catalogue
    /// knows of a model is filled from it, and the models it does not know
    /// (embeddings, audio, a local server's own) pass through, the speech
    /// models tagged.
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

    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        let answered = self.answer(request).await;
        answered.map_err(|failure| self.exit.leave(&failure))
    }

    /// The text of [`generate_structured_stream`], as it comes: its
    /// reasoning, its calls and its end are left out.
    ///
    /// [`generate_structured_stream`]: AiProvider::generate_structured_stream
    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        stream::text(self.generate_structured_stream(request))
    }

    /// The events of one generation, as they come: the text and the
    /// reasoning (a `<think>` block that opens the answer, or a reasoning
    /// field), the composition of each call, then the whole calls and the
    /// done event. An answer constrained by a response schema is checked
    /// before its done event, which its failure replaces.
    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        let checked = checked_stream(stream::events(self, request), request, NAME, refusal);
        stream::leaving(checked, &self.exit)
    }
}

/// The waits a replayed provider did not take between its tries (see
/// `OpenAiAiProvider::replaying`).
#[cfg(feature = "replay")]
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct ReplayWaits(Arc<std::sync::Mutex<Vec<f64>>>);

#[cfg(feature = "replay")]
impl ReplayWaits {
    fn keep(&self, seconds: f64) {
        let mut waits = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        waits.push(seconds);
    }

    /// The waits so far, in seconds, in the order they were asked.
    #[must_use]
    pub fn taken(&self) -> Vec<f64> {
        let waits = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        waits.clone()
    }
}

impl fmt::Debug for OpenAiAiProvider {
    /// The configuration's `Debug` form: neither the key nor what a base
    /// URL holds beside its scheme, host and port.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiAiProvider")
            .field("config", &self.config)
            .field("own_endpoint", &self.own_endpoint)
            .field("timeouts", &self.timeouts)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
