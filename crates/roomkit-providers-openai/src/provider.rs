//! The provider: `OpenAI`'s Chat Completions, and the servers that speak
//! it behind a base URL.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reqwest::header::HeaderMap;
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::{Call, Client, Exit, Secrets, Settings, Timeouts, tried_again};
use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, ModelInfo, ProviderError, ProviderStream, StreamDone,
    TextStream, checked_stream, listing,
};

use self::build::{headers_of, own_endpoint, secrets_of};
use crate::answer;
use crate::config::OpenAiConfig;
use crate::endpoint::{OPENAI_BASE_URL, is_openai_endpoint};
use crate::failure::{self, Failure};
use crate::models;
use crate::request::Asked;
use crate::stream;

mod build;

/// The provider's name in its failures: Python's `_provider_name`.
const NAME: &str = "openai";

/// What the provider is called: the name of Python's class, which
/// `AIProvider.name` answers.
const CLASS_NAME: &str = "OpenAIAIProvider";

/// The path of Chat Completions under a base URL.
const CHAT_COMPLETIONS: &str = "/chat/completions";

/// The path of the model listing under a base URL.
const MODELS: &str = "/models";

/// Where the retry rule of Python's openai SDK stands: a server may ask for
/// a wait of two minutes at most, and an answer that asks for more is
/// final.
const RETRY: Rule = Rule {
    longest_asked: 120.0,
    past_it: Past::Final,
};

/// What a request asks for.
const JSON: &str = "application/json";

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
/// - no failure's text repeats a secret of the configuration or of the
///   request that a server's answer holds, as it is or as the code that
///   builds the failure writes it again: the key, the value of a default
///   header, the user information of the base URL, and the key a host
///   lends one request
///   ([`API_KEY_METADATA_KEY`](roomkit_providers_llm::API_KEY_METADATA_KEY)),
///   which the provider does not send. This holds whatever built the
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
/// - a redirect is not followed, a proxy's own among them, and no
///   compressed answer is asked for;
/// - the environment is read once, when the provider is built, for the
///   network of its host and nothing else: the proxies `HTTP_PROXY`,
///   `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` name, and the certificate
///   authorities `SSL_CERT_FILE` or `SSL_CERT_DIR` name, the system's
///   otherwise, as Python's client reads them. No key and no URL is read
///   from it. A proxy's credentials appear in no failure;
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
pub struct OpenAi {
    pub(crate) config: Arc<OpenAiConfig>,
    /// Where a completion is asked.
    pub(crate) chat: reqwest::Url,
    /// Where the models are listed.
    pub(crate) models: reqwest::Url,
    pub(crate) headers: HeaderMap,
    /// The client every request is sent with: its tries, its timeouts and
    /// the bounds of what it reads.
    pub(crate) client: Client,
    own_endpoint: bool,
    left_out_warned: Arc<AtomicBool>,
    /// Where every failure leaves, cleaned of the configuration's secrets:
    /// the failure of a generation with its request, whose own key it is
    /// then cleaned of too.
    exit: Arc<Exit>,
}

impl OpenAi {
    /// The provider for `config`.
    ///
    /// # Errors
    ///
    /// A failure before any request: a base URL that is no `http` or
    /// `https` URL, or that has a query or a fragment, which the path of
    /// the API cannot follow; a default header that is no HTTP header; an
    /// environment whose proxy variables do not read or name a proxy that
    /// is neither `http` nor `https`, or whose `SSL_CERT_FILE` names a file
    /// that holds no certificate; or a client that could not be built.
    pub fn new(config: OpenAiConfig) -> Result<Self, ProviderError> {
        // A provider that cannot be built says why in its own words.
        let nothing_to_hide = Exit::new(NAME, Secrets::default());
        Self::built(config).map_err(|failure| nothing_to_hide.leave(&failure))
    }

    fn built(config: OpenAiConfig) -> Result<Self, Failure> {
        let base_url = config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
        let chat = joined(base_url, CHAT_COMPLETIONS)?;
        let models = joined(base_url, MODELS)?;
        let headers = headers_of(&config, &chat)?;
        let client = Client::new(Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: config.max_retries,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::openai"),
        })?;
        let exit = Exit::new(NAME, secrets_of(&config, &client, &chat));
        Ok(Self {
            own_endpoint: own_endpoint(&config, &chat),
            config: Arc::new(config),
            chat,
            models,
            headers,
            client,
            left_out_warned: Arc::new(AtomicBool::new(false)),
            exit: Arc::new(exit),
        })
    }

    /// The call that asks a completion, the body of its request `body`.
    pub(crate) fn asking<'a>(&'a self, body: &'a [u8]) -> Call<'a> {
        Call {
            url: &self.chat,
            headers: &self.headers,
            body: Some(body),
        }
    }

    /// The call that asks the models the endpoint lists.
    fn asking_the_models(&self) -> Call<'_> {
        Call {
            url: &self.models,
            headers: &self.headers,
            body: None,
        }
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
        let urls = [&mut self.chat, &mut self.models];
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
        let moved = roomkit_providers_client::replayed_at([&mut self.chat, &mut self.models], fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
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
        let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
        let (answer, json) = self.client.send(self.asking(&body)).await?;
        answer::read(&answer, json, request, &self.config.model, NAME)
    }

    /// The models the endpoint lists, each by its id alone, as Python's
    /// `list_models` reads the page.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let (page, json) = self.client.send(self.asking_the_models()).await?;
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

impl AiProvider for OpenAi {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn label(&self) -> &'static str {
        NAME
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

    /// One whole answer. Its failure leaves with `request`: the key a host
    /// lends the request, which the provider does not send, is in no
    /// failure either.
    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        let answered = self.answer(request).await;
        answered.map_err(|failure| self.exit.leave_for(request, &failure))
    }

    /// The text of [`generate_structured_stream`], as it comes: its
    /// reasoning, its calls and its end are left out.
    ///
    /// [`generate_structured_stream`]: AiProvider::generate_structured_stream
    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        roomkit_providers_client::stream::text(self.generate_structured_stream(request))
    }

    /// The events of one generation, as they come: the text and the
    /// reasoning (a `<think>` block that opens the answer, or a reasoning
    /// field), the composition of each call, then the whole calls and the
    /// done event. An answer constrained by a response schema is checked
    /// before its done event, which its failure replaces. A failure leaves
    /// with `request`, as that of [`generate`](AiProvider::generate).
    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        let checked = checked_stream(stream::events(self, request), request, NAME, refusal);
        roomkit_providers_client::stream::leaving(checked, &self.exit, request)
    }
}

/// The waits a replayed provider did not take between its tries (see
/// `OpenAi::replaying`).
#[cfg(feature = "replay")]
#[doc(hidden)]
pub use roomkit_providers_client::ReplayWaits;

impl fmt::Debug for OpenAi {
    /// The configuration's `Debug` form: neither the key nor what a base
    /// URL holds beside its scheme, host and port.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAi")
            .field("config", &self.config)
            .field("own_endpoint", &self.own_endpoint)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
