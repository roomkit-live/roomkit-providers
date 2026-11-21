//! The provider: `OpenAI`'s Chat Completions, and the servers that speak
//! it behind a base URL.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, ProviderErrorOptions,
};

use crate::answer;
use crate::config::OpenAiConfig;
use crate::endpoint::{OPENAI_BASE_URL, is_openai_endpoint};
use crate::failure::Failing;
use crate::models;
use crate::request::Asked;
use crate::retry;

/// The provider's name in its failures: Python's `_provider_name`.
const NAME: &str = "openai";

/// What the provider is called: the name of Python's class, which
/// `AIProvider.name` answers.
const CLASS_NAME: &str = "OpenAIAIProvider";

/// The path of Chat Completions under a base URL.
const CHAT_COMPLETIONS: &str = "/chat/completions";

/// The most bytes of an answer the provider reads: a server that sends
/// more fails the request.
pub(crate) const ANSWER_LIMIT: usize = 4 * 1024 * 1024;

/// A wait the provider takes between two tries.
type Wait = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The clock a `retry-after` date is read against, and the wait between
/// two tries: the system's, which a test replaces.
#[derive(Clone)]
pub(crate) struct Pace {
    /// The seconds since 1970.
    pub(crate) now: Arc<dyn Fn() -> f64 + Send + Sync>,
    /// Waits before the next try.
    pub(crate) wait: Arc<dyn Fn(Duration) -> Wait + Send + Sync>,
}

impl Default for Pace {
    fn default() -> Self {
        let now = || {
            let since = SystemTime::now().duration_since(UNIX_EPOCH);
            since.map_or(0.0, |since| since.as_secs_f64())
        };
        Self {
            now: Arc::new(now),
            wait: Arc::new(|wait| Box::pin(tokio::time::sleep(wait))),
        }
    }
}

/// Compiles only while the URL the client dials is the `url` crate's, the
/// one `roomkit-providers-http` validates a destination with: a URL that
/// was checked there is read the same way here.
const fn the_client_s_url(url: reqwest::Url) -> url::Url {
    url
}

/// An AI provider for `OpenAI`'s Chat Completions, and for the servers that
/// speak it behind a base URL: Python's `OpenAIAIProvider`.
///
/// It answers a request whole ([`generate`](AiProvider::generate)); its
/// streams are not written yet. A request is sent to the base URL of the
/// [configuration](OpenAiConfig), `OpenAI`'s own when it names none, with
/// the configuration's key as its bearer token.
///
/// What it guarantees:
///
/// - the key appears in no failure, log, `Debug` form or result: a failure
///   that echoes a server's answer has the key removed from it;
/// - what a failure echoes of a server's answer is within 500 characters,
///   on one line, and an answer past 4 MiB fails the request;
/// - a redirect is not followed, and no proxy, key or URL is read from the
///   environment;
/// - the timeouts are the configuration's: `connect_timeout` to connect,
///   `timeout` between two reads.
#[derive(Clone)]
pub struct OpenAiAiProvider {
    config: Arc<OpenAiConfig>,
    endpoint: reqwest::Url,
    headers: HeaderMap,
    client: reqwest::Client,
    own_endpoint: bool,
    left_out_warned: Arc<AtomicBool>,
    pub(crate) pace: Pace,
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
        let endpoint = joined(config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL))?;
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
            own_endpoint: own_endpoint(&config, &endpoint),
            config: Arc::new(config),
            endpoint,
            headers,
            client,
            left_out_warned: Arc::new(AtomicBool::new(false)),
            pace: Pace::default(),
        })
    }

    /// The URL the provider sends its requests to.
    #[cfg(test)]
    pub(crate) fn endpoint(&self) -> &reqwest::Url {
        &self.endpoint
    }

    /// Sends the requests to `url` instead: a test's fake of the endpoint.
    /// What the provider knows of its endpoint stays what its
    /// configuration says.
    #[cfg(test)]
    pub(crate) fn point_at(&mut self, url: &str) {
        if let Ok(url) = reqwest::Url::parse(url) {
            self.endpoint = url;
        }
    }

    /// What the provider's failures carry: its name, and the key to keep
    /// out of them.
    fn failing(&self) -> Failing<'_> {
        Failing {
            provider: NAME,
            secret: &self.config.api_key,
        }
    }

    /// What a request is written for.
    fn asked(&self) -> Asked<'_> {
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
        let body = self.asked().body(request)?;
        let body = serde_json::to_vec(&body)
            .map_err(|_| refused("the request could not be written as JSON"))?;
        let (answer, json) = self.send(&body).await?;
        answer::read(&answer, json, request, &self.config.model, self.failing())
    }

    /// Sends `body` until an answer comes or no try is left: the answer's
    /// body, and whether the server said it is JSON.
    async fn send(&self, body: &[u8]) -> Result<(Vec<u8>, bool), ProviderError> {
        let mut retries_taken = 0;
        loop {
            let left = retries_taken < self.config.max_retries;
            match self.attempt(body, retries_taken, left).await {
                Ok(answered) => return Ok(answered),
                Err(Failed::Final(error)) => return Err(error),
                Err(Failed::Again(wait)) => {
                    tracing::info!(
                        target: "roomkit::providers::openai",
                        retries_taken,
                        ?wait,
                        "a request is tried again"
                    );
                    (self.pace.wait)(wait).await;
                    retries_taken += 1;
                }
            }
        }
    }

    /// One try: the answer, the failure that ends the request, or the wait
    /// before another try when one is `left`.
    async fn attempt(
        &self,
        body: &[u8],
        retries_taken: u32,
        left: bool,
    ) -> Result<(Vec<u8>, bool), Failed> {
        let now = (self.pace.now)();
        let failing = self.failing();
        let again =
            |headers: Option<&HeaderMap>| Failed::Again(retry::delay(retries_taken, headers, now));
        let lost = |error: &reqwest::Error| {
            if left {
                again(None)
            } else {
                Failed::Final(failing.transport(error))
            }
        };
        let call = self.client.post(self.endpoint.clone());
        let call = call.headers(self.headers.clone()).body(body.to_vec());
        let response = match call.send().await {
            Ok(response) => response,
            // A request that could not be built never left: it fails the
            // same however many times it is tried.
            Err(error) if error.is_builder() => {
                return Err(Failed::Final(failing.transport(&error)));
            }
            Err(error) => return Err(lost(&error)),
        };
        let status = response.status();
        if !status.is_success() {
            if left && retry::should_retry(status, response.headers(), now) {
                return Err(again(Some(response.headers())));
            }
            // An error body past what a request reads still keeps its
            // status, as Python's client reads the status whatever the body.
            let body = match read_body(response).await {
                Ok(body) => body,
                Err(Unread::TooLong) => Vec::new(),
                Err(Unread::Lost(error)) => return Err(lost(&error)),
            };
            return Err(Failed::Final(failing.status(status.as_u16(), &body)));
        }
        let json = is_json(&response);
        match read_body(response).await {
            Ok(body) => Ok((body, json)),
            Err(Unread::Lost(error)) => Err(lost(&error)),
            Err(Unread::TooLong) => {
                let message = format!("the provider's answer is longer than {ANSWER_LIMIT} bytes");
                Err(Failed::Final(failing.unreadable(&message)))
            }
        }
    }
}

/// How a try ended without an answer.
enum Failed {
    /// The request failed for good.
    Final(ProviderError),
    /// Another try comes after this wait.
    Again(Duration),
}

/// Why an answer's body was not read.
enum Unread {
    /// It is past [`ANSWER_LIMIT`].
    TooLong,
    /// The connection failed while it was read.
    Lost(reqwest::Error),
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

/// The URL of Chat Completions under `base_url`, which the failure of a
/// refused one does not repeat: it may carry credentials.
fn joined(base_url: &str) -> Result<reqwest::Url, ProviderError> {
    let not_http = || refused("its base URL is not an http or https URL");
    let base = reqwest::Url::parse(base_url).map_err(|_| not_http())?;
    if base.query().is_some() || base.fragment().is_some() {
        return Err(refused(
            "its base URL has a query or a fragment, which the path of the API cannot follow",
        ));
    }
    let joined = format!("{}{CHAT_COMPLETIONS}", base_url.trim_end_matches('/'));
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
/// as Python's client lets it. An empty key sends no `Authorization`.
fn headers_of(config: &OpenAiConfig) -> Result<HeaderMap, ProviderError> {
    let json = HeaderValue::from_static("application/json");
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, json.clone());
    headers.insert(CONTENT_TYPE, json);
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

/// Whether the server says its answer is JSON.
fn is_json(response: &reqwest::Response) -> bool {
    let kind = response.headers().get(CONTENT_TYPE);
    let kind = kind.and_then(|value| value.to_str().ok());
    kind.is_some_and(|value| value.trim_start().starts_with("application/json"))
}

/// The body of an answer, within [`ANSWER_LIMIT`].
async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, Unread> {
    let declared = response.content_length().unwrap_or_default();
    if usize::try_from(declared).map_or(true, |declared| declared > ANSWER_LIMIT) {
        return Err(Unread::TooLong);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Unread::Lost)? {
        if body.len().saturating_add(chunk.len()) > ANSWER_LIMIT {
            return Err(Unread::TooLong);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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

    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        self.answer(request).await
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
