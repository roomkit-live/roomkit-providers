//! The Ollama provider: Python's `OllamaAIProvider`, on Ollama's native
//! API.
//!
//! A whole answer is asked whole (`stream: false`), a stream as JSON lines.
//! A request is sent once: the ollama SDK RoomKit's provider runs on tries
//! no request again. The listing reads the models the server holds, then
//! asks each one's capabilities, one after the other.

use std::fmt;
use std::sync::Arc;

use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use roomkit::model::Metadata;
use roomkit_ai::{
    AiContext, AiProvider, AiResponse, CallIds, ModelInfo, ProviderError, ProviderStream,
    StreamDone, TextStream, check_schema_answer, checked_stream, listing,
};
use roomkit_providers_client::endpoint::joined;
use roomkit_providers_client::retry::{Past, Rule};
use roomkit_providers_client::stream::{leaving, text, timed};
use roomkit_providers_client::{Call, Client, Exit, Settings, Timeouts, tried_again};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::OllamaConfig;
use crate::failure::{self, Failure, past_mark};
use crate::host::base_url;
use crate::models;
use crate::reading::Answer;
use crate::request::Asked;
use crate::stream;

/// The provider its failures name.
const LABEL: &str = "ollama";

/// The provider's name, as Python's class names itself.
const CLASS_NAME: &str = "OllamaAIProvider";

/// The variable the SDK reads a key from, where the configuration states
/// no authorization.
const KEY_VARIABLE: &str = "OLLAMA_API_KEY";

/// No retry is taken (the SDK takes none): the rule only bounds what a
/// server may ask.
const RETRY: Rule = Rule {
    longest_asked: 60.0,
    past_it: Past::Longest,
};

/// A model an Ollama server holds (RFC §6.7): Python's
/// `OllamaAIProvider(config)`. A turn's reasoning goes as Ollama's `think`,
/// read back from the answer's `thinking`; a tool's schema goes whole; an
/// image goes as base64, a path read from the disk.
#[derive(Clone)]
pub struct Ollama {
    pub(crate) config: Arc<OllamaConfig>,
    pub(crate) chat: reqwest::Url,
    tags: reqwest::Url,
    show: reqwest::Url,
    pub(crate) headers: HeaderMap,
    pub(crate) client: Client,
    exit: Arc<Exit>,
}

impl Ollama {
    /// The provider `config` configures. The environment is read then, and
    /// only then: for the proxies and the certificate authorities, as every
    /// provider reads them, and for `OLLAMA_API_KEY`, the key the SDK sends
    /// where the configuration states no authorization.
    ///
    /// # Errors
    ///
    /// A failure before any request: a host no URL reads from, a header
    /// that cannot be sent, a timeout the clock cannot count.
    pub fn new(config: OllamaConfig) -> Result<Self, ProviderError> {
        let from_the_environment = std::env::var(KEY_VARIABLE).ok();
        Self::built(config, from_the_environment).map_err(|failure| Exit::unbuilt(LABEL, &failure))
    }

    fn built(config: OllamaConfig, from_the_environment: Option<String>) -> Result<Self, Failure> {
        let base = base_url(&config.host).ok_or_else(|| Failure::unbuilt("its host is no URL"))?;
        let chat = joined(&base, "/api/chat")?;
        let tags = joined(&base, "/api/tags")?;
        let show = joined(&base, "/api/show")?;
        let (headers, secrets) = headers(&config, from_the_environment)?;
        let client = Client::new(Settings {
            timeouts: Timeouts::new(config.timeout, config.connect_timeout)?,
            max_retries: 0,
            retry: RETRY,
            refused: failure::status,
            tried_again: tried_again!("roomkit::providers::ollama"),
        })?;
        let exit = client.exit(LABEL, secrets, [], &chat);
        Ok(Self {
            config: Arc::new(config),
            chat,
            tags,
            show,
            headers,
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
        let urls = [&mut self.chat, &mut self.tags, &mut self.show];
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
        let urls = [&mut self.chat, &mut self.tags, &mut self.show];
        let moved = roomkit_providers_client::replayed_at(urls, fake);
        moved.map_err(|failure| self.exit.leave(&failure))?;
        let waits = self.client.replaying(clock);
        Ok((self, waits))
    }

    /// The answer to `request`, asked whole (Python's `generate`).
    async fn answer(&self, request: &AiContext) -> Result<AiResponse, Failure> {
        let body = self.asked().body(request, false).await?;
        let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
        let call = Call {
            url: &self.chat,
            headers: &self.headers,
            body: Some(&body),
            token: None,
        };
        let (answer, _) = self.client.send(call).await?;
        let answer: Value = serde_json::from_slice(past_mark(&answer))
            .map_err(|error| Failure::malformed(&error))?;
        let answer = Answer::deserialize(&answer).map_err(|error| failure::unread(&error))?;
        let message = &answer.message;
        let content = message.content.clone().unwrap_or_default();
        let finish_reason = answer.finish_reason();
        let calls = message.calls(&mut CallIds::new());
        if let Some(schema) = request
            .response_schema
            .as_ref()
            .filter(|_| calls.is_empty())
        {
            check_schema_answer(&content, schema, LABEL, None, finish_reason)?;
        }
        let mut response = AiResponse::new(content);
        response.thinking = message
            .thinking
            .clone()
            .filter(|thinking| !thinking.is_empty());
        response.finish_reason = finish_reason;
        response.usage = answer.usage();
        let model = answer.answered_by().unwrap_or(&self.config.model);
        let mut metadata = Metadata::new();
        metadata.insert("model".to_owned(), model.into());
        response.metadata = metadata;
        response.tool_calls = calls;
        Ok(response)
    }

    /// The models the server holds, each with the capabilities it answers
    /// of them, none where it answers nothing the SDK reads.
    async fn live_models(&self) -> Result<Vec<ModelInfo>, Failure> {
        let call = Call {
            url: &self.tags,
            headers: &self.headers,
            body: None,
            token: None,
        };
        let (page, json) = self.client.send(call).await?;
        let page: Value = match serde_json::from_slice(past_mark(&page)) {
            Ok(page) => page,
            Err(error) if json => return Err(Failure::malformed(&error)),
            Err(_) => return Err(failure::no_listing()),
        };
        let names = models::installed(&page).ok_or_else(failure::no_listing)?;
        let mut live = Vec::with_capacity(names.len());
        for name in names {
            let mut model = ModelInfo::new(name.as_str());
            model.capabilities = self.capabilities(&name).await;
            live.push(model);
        }
        Ok(live)
    }

    /// What `/api/show` answers of the capabilities of the model `name`:
    /// none for a failure, as Python reads any.
    async fn capabilities(&self, name: &str) -> Vec<String> {
        let Ok(body) = serde_json::to_vec(&json!({"model": name})) else {
            return Vec::new();
        };
        let call = Call {
            url: &self.show,
            headers: &self.headers,
            body: Some(&body),
            token: None,
        };
        let Ok((shown, _)) = self.client.send(call).await else {
            return Vec::new();
        };
        serde_json::from_slice(past_mark(&shown))
            .map_or_else(|_| Vec::new(), |shown| models::capabilities(&shown))
    }
}

/// The headers of every request, as the SDK sets them, and the secrets no
/// failure may repeat: the configured headers, the SDK's own over them, the
/// key over a configured authorization, and where the configuration states
/// none, the key `OLLAMA_API_KEY` holds.
fn headers(
    config: &OllamaConfig,
    from_the_environment: Option<String>,
) -> Result<(HeaderMap, Vec<String>), Failure> {
    let unsent = || Failure::unbuilt("a header cannot be sent");
    let mut headers = HeaderMap::new();
    let mut secrets = Vec::new();
    for (name, value) in config.headers.iter().flatten() {
        let name =
            HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()).map_err(|_| unsent())?;
        // The SDK's own user-agent is sent over a configured one, as its
        // content type and its accept are, set below.
        if name == USER_AGENT {
            continue;
        }
        let mut value = HeaderValue::from_str(value).map_err(|_| unsent())?;
        value.set_sensitive(true);
        secrets.push(value.to_str().unwrap_or_default().to_owned());
        headers.insert(name, value);
    }
    let key = config.api_key.clone();
    let key = key.or_else(|| {
        let stated = headers.contains_key(AUTHORIZATION);
        from_the_environment.filter(|key| !key.is_empty() && !stated)
    });
    if let Some(key) = key {
        let mut bearer = HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| unsent())?;
        bearer.set_sensitive(true);
        headers.insert(AUTHORIZATION, bearer);
        secrets.push(key);
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    Ok((headers, secrets))
}

/// Ollama states no refusal of its own.
fn no_refusal(_done: &StreamDone) -> Option<String> {
    None
}

impl AiProvider for Ollama {
    fn name(&self) -> &'static str {
        CLASS_NAME
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }

    /// Images go through whatever the model: the server refuses a model
    /// that reads none.
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
        let answer = self.answer(request).await;
        answer.map_err(|failure| self.exit.leave_for(request, &failure))
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

impl fmt::Debug for Ollama {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ollama")
            .field("config", &self.config)
            .field("timeouts", &self.client.timeouts())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: the ollama SDK's BaseClient: OLLAMA_API_KEY is sent where no
    // authorization is configured, and only then.
    #[test]
    fn the_environment_s_key_goes_where_no_authorization_is_configured() {
        let bearer = |config: &OllamaConfig, environment: Option<&str>| {
            let (headers, _) = headers(config, environment.map(str::to_owned)).unwrap();
            headers
                .get(AUTHORIZATION)
                .map(|value| value.to_str().unwrap().to_owned())
        };
        let plain = OllamaConfig::default();
        assert_eq!(bearer(&plain, Some("e")).as_deref(), Some("Bearer e"));
        assert_eq!(bearer(&plain, Some("")), None);
        assert_eq!(bearer(&plain, None), None);
        let keyed = OllamaConfig {
            api_key: Some("k".into()),
            ..OllamaConfig::default()
        };
        assert_eq!(bearer(&keyed, Some("e")).as_deref(), Some("Bearer k"));
        let basic = OllamaConfig {
            headers: Some(vec![("Authorization".into(), "Basic x".into())]),
            ..OllamaConfig::default()
        };
        assert_eq!(bearer(&basic, Some("e")).as_deref(), Some("Basic x"));
    }
}
