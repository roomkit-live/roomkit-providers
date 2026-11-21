//! The provider: `OpenAI`'s Chat Completions, and the servers that speak
//! it behind a base URL.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, ProviderErrorOptions,
    ProviderStream, StreamDone, TextStream, checked_stream, listing,
};

use roomkit_providers_http::sse::STREAM_LIMIT;

use crate::answer;
use crate::config::OpenAiConfig;
use crate::endpoint::{OPENAI_BASE_URL, is_openai_endpoint};
use crate::exchange::{Call, Pace};
use crate::failure::Failing;
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

/// Compiles only while the URL the client dials is the `url` crate's, the
/// one `roomkit-providers-http` validates a destination with: a URL that
/// was checked there is read the same way here.
const fn the_client_s_url(url: reqwest::Url) -> url::Url {
    url
}

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
/// - the key appears in no failure, log, `Debug` form or result: a failure
///   that echoes a server's answer has the key removed from it;
/// - a failure echoes of a server's answer only what an error object says
///   of itself (its message, its type, its code), within 500 characters, on
///   one line; a body that is no JSON is never echoed;
/// - every size a server controls is bounded: an answer read whole past
///   4 MiB fails the request; a stream fails at a line or an event past
///   1 MiB, and past 256 MiB in all;
/// - a redirect is not followed, and no proxy, key or URL is read from the
///   environment;
/// - the timeouts are the configuration's: `connect_timeout` to connect,
///   `timeout` between two reads, which also ends a stream that stalls.
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
        let base_url = config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
        let chat = joined(base_url, CHAT_COMPLETIONS)?;
        let models = joined(base_url, MODELS)?;
        let headers = headers_of(&config)?;
        let tls = roomkit_providers_http::tls_config().map_err(|_| refused("TLS is not set"))?;
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .no_gzip()
            // Neither the system's proxies nor its variables are read.
            .no_proxy()
            .read_timeout(config.timeout)
            .connect_timeout(config.connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| refused("the HTTP client could not be built"))?;
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
        })
    }

    /// The URL the provider asks its completions at.
    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &reqwest::Url {
        &self.chat
    }

    /// Sends the requests to `address` instead, over plain HTTP, each to
    /// the path it had: a test's fake of the endpoint. What the provider
    /// knows of its endpoint stays what its configuration says.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, address: std::net::SocketAddr) {
        for url in [&mut self.chat, &mut self.models] {
            let target = &url[url::Position::BeforePath..];
            if let Ok(pointed) = reqwest::Url::parse(&format!("http://{address}{target}")) {
                *url = pointed;
            }
        }
    }

    /// What the provider's failures carry: its name, and the key to keep
    /// out of them.
    pub(crate) fn failing(&self) -> Failing<'_> {
        Failing {
            provider: NAME,
            secret: &self.config.api_key,
        }
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
    async fn answer(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        let body = self.asked().body(request, false)?;
        let body = serde_json::to_vec(&body).map_err(|_| unwritten())?;
        let (answer, json) = self.send(Call::Chat(&body)).await?;
        answer::read(&answer, json, request, &self.config.model, self.failing())
    }

    /// The models the endpoint lists, each by its id alone, as Python's
    /// `list_models` reads the page.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let (page, json) = self.send(Call::Models).await?;
        let no_page = || {
            let what = "the model listing is no page of models with their ids";
            self.failing().unreadable(what)
        };
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
pub(crate) fn unwritten() -> ProviderError {
    let options = ProviderErrorOptions {
        provider: NAME.to_owned(),
        before_request: true,
        ..ProviderErrorOptions::default()
    };
    ProviderError::new("the request could not be written as JSON", options)
}

/// A failure before any request, final.
fn refused(reason: &str) -> ProviderError {
    let options = ProviderErrorOptions {
        provider: NAME.to_owned(),
        before_request: true,
        ..ProviderErrorOptions::default()
    };
    ProviderError::new(format!("the provider cannot be built: {reason}"), options)
}

/// The URL of `path` under `base_url`, which the failure of a refused one
/// does not repeat: it may carry credentials.
fn joined(base_url: &str, path: &str) -> Result<reqwest::Url, ProviderError> {
    let not_http = || refused("its base URL is not an http or https URL");
    let base = reqwest::Url::parse(base_url).map_err(|_| not_http())?;
    if base.query().is_some() || base.fragment().is_some() {
        return Err(refused(
            "its base URL has a query or a fragment, which the path of the API cannot follow",
        ));
    }
    let joined = format!("{}{path}", base_url.trim_end_matches('/'));
    let url = reqwest::Url::parse(&joined).map_err(|_| not_http())?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(not_http()),
    }
}

/// Whether the provider talks to `OpenAI`'s own endpoint, whose models and
/// rules it knows: the base URL is `OpenAI`'s as Python compares it, and
/// the URL the client dials is `OpenAI`'s as the client reads it.
fn own_endpoint(config: &OpenAiConfig, dialed: &reqwest::Url) -> bool {
    let official = the_client_s_url(dialed.clone());
    is_openai_endpoint(config.base_url.as_deref())
        && official.scheme() == "https"
        && official.host_str() == Some("api.openai.com")
        && official.port_or_known_default() == Some(443)
        && official.path() == "/v1/chat/completions"
        && official.username().is_empty()
        && official.password().is_none()
}

/// The headers every request carries: what the provider writes, then the
/// configuration's, one of which replaces the provider's of the same name
/// as Python's client lets it. An empty key sends no `Authorization`. A
/// request with a body says beside them what the body is.
fn headers_of(config: &OpenAiConfig) -> Result<HeaderMap, ProviderError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static(JSON));
    if !config.api_key.is_empty() {
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key));
        let mut bearer = bearer.map_err(|_| refused("its key cannot go in a header"))?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
    }
    for (name, value) in config.default_headers.iter().flatten() {
        let no_header = || refused("one of its default headers is no HTTP header");
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| no_header())?;
        let mut value = HeaderValue::from_str(value).map_err(|_| no_header())?;
        value.set_sensitive(name == AUTHORIZATION);
        headers.insert(name, value);
    }
    Ok(headers)
}

impl AiProvider for OpenAiAiProvider {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// What the configuration says: `vision`.
    fn supports_vision(&self) -> bool {
        self.config.vision
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
        let live = self.live_models().await?;
        Ok(listing(live, models::models()))
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_streaming(&self) -> bool {
        true
    }

    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        self.answer(request).await
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
        checked_stream(stream::events(self, request), request, NAME, refusal)
    }
}

impl fmt::Debug for OpenAiAiProvider {
    /// The configuration's `Debug` form: neither the key nor what a base
    /// URL holds beside its scheme, host and port.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiAiProvider")
            .field("config", &self.config)
            .field("own_endpoint", &self.own_endpoint)
            .finish_non_exhaustive()
    }
}
