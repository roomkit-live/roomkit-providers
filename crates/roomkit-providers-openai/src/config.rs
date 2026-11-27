//! What an `OpenAI` provider is configured with: Python's `OpenAIConfig`.

pub(crate) mod form;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use roomkit::model::Metadata;
use roomkit_providers_client::TokenSource;

use crate::endpoint::{OPENAI_BASE_URL, is_openai_endpoint, shown_url};

/// The models that take their token bound as `max_completion_tokens` on
/// `OpenAI`'s own endpoint (Python's `_MAX_COMPLETION_TOKEN_MODEL_PREFIXES`).
const MAX_COMPLETION_TOKEN_MODELS: [&str; 6] = ["gpt-4.1", "gpt-5", "gpt-6", "o1", "o3", "o4"];

/// The models that take no temperature on `OpenAI`'s own endpoint (Python's
/// `_FIXED_TEMPERATURE_MODEL_PREFIXES`).
const FIXED_TEMPERATURE_MODELS: [&str; 5] = ["gpt-5", "gpt-6", "o1", "o3", "o4"];

/// The HTTP API spoken by the provider. Selection is explicit; no fallback
/// changes protocols or repeats a failed generation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenAiApi {
    /// OpenAI-compatible Chat Completions, including local endpoints.
    #[default]
    ChatCompletions,
    /// Stateless Responses with streamed text, function tools and reasoning replay.
    Responses,
}

impl OpenAiApi {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/chat/completions",
            Self::Responses => "/responses",
        }
    }
}

/// The endpoint a provider calls and how: Python's `OpenAIConfig(*, api_key,
/// base_url=None, model, max_tokens=1024, timeout=30.0, connect_timeout=5.0,
/// max_retries=0, include_stream_usage=True, use_max_completion_tokens=False,
/// supports_custom_temperature=True, reasoning_effort=None,
/// default_headers=None, extra_body=None, supports_response_schema=None,
/// supports_response_schema_with_tools=None,
/// supports_reasoning_effort_with_tools=None, thinking_field=None)`, built
/// with [`OpenAiConfig::new`] and struct update.
///
/// ```
/// use std::time::Duration;
///
/// use roomkit_providers_openai::OpenAiConfig;
///
/// let config = OpenAiConfig {
///     base_url: Some("http://localhost:11434/v1".to_owned()),
///     timeout: Duration::from_secs(120),
///     ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
/// };
/// assert_eq!(config.max_tokens, Some(1024));
/// // Neither the key nor what follows the host of the base URL is shown.
/// let shown = format!("{config:?}");
/// assert!(shown.contains("http://localhost:11434") && !shown.contains("placeholder"));
/// ```
///
/// # JSON form
///
/// The configuration reads and writes the JSON that Python's
/// `OpenAIConfig` gives with `model_dump(mode="json")`: the same members,
/// in the same order, the timeouts in seconds. As in Python, the key is
/// written masked, `**********`, an empty one as an empty text: a
/// configuration written then read back holds the mask, not the key.
/// [`vision`](Self::vision), which Python's configuration does not have, is
/// no part of the form: it is not written, and a configuration that is read
/// has it off.
///
/// The two switches Python profiles are written as Python writes them,
/// decided: `true` or `false`. A member left out of what is read leaves the
/// switch unset, and a `true` or a `false` states it.
///
/// Input is stricter than Python's. Each of these is refused here and taken
/// by Python:
///
/// - a value pydantic would convert: `"30"` for a number, `1024.0` for an
///   integer, `1` for a boolean;
/// - a `max_tokens` or a `max_retries` outside 32 bits without a sign;
/// - a `timeout` or a `connect_timeout` below zero, or past what a
///   [`Duration`] holds; one is kept to the nanosecond.
///
/// `default_headers` are written sorted by name, where Python keeps the
/// order they were given in.
///
/// Two more differences come from the JSON parser, `serde_json`, where
/// Python reads with its `json` module:
///
/// - a member written twice is refused, where Python keeps the last:
///   `{"model": "a", "model": "b"}`. This holds when the text itself is
///   read: a `serde_json::Value` has already kept the last;
/// - an integer of `extra_body` outside 64 bits is read as a float:
///   `18446744073709551616` reads as `1.8446744073709552e19`.
///
/// The form is not for showing: like Python's, it writes a default header's
/// value and the credentials a base URL carries whole. Only the key is
/// masked. The `Debug` form hides all three.
///
/// # Debug
///
/// The `Debug` form never prints the API key, nor the base URL's user
/// information, path or query, which may carry credentials, nor a default
/// header's value. Of the base URL it shows the origin the HTTP client
/// dials for it, read as the client's own URL parser reads it.
#[derive(Clone)]
pub struct OpenAiConfig {
    /// The key a request carries as its bearer token.
    pub api_key: String,
    /// The endpoint's base URL; `None` calls `OpenAI`'s own.
    pub base_url: Option<String>,
    /// The model asked.
    pub model: String,
    /// The most tokens a generation writes, unless its request says;
    /// `None` sends no bound, and the endpoint decides.
    pub max_tokens: Option<u32>,
    /// How long a request waits for the endpoint between two reads
    /// (Python's `timeout`, in seconds there).
    pub timeout: Duration,
    /// How long a request waits for its connection (Python's
    /// `connect_timeout`, in seconds there).
    pub connect_timeout: Duration,
    /// How many times a request is tried again on its own. 0 by default: a
    /// host's retry policy is the layer that retries, with its fallback.
    pub max_retries: u32,
    /// Whether a streamed answer reports its usage
    /// (`stream_options.include_usage`).
    pub include_stream_usage: bool,
    /// Whether the token bound is sent as `max_completion_tokens`, as
    /// `OpenAI`'s reasoning models take it, rather than `max_tokens`.
    /// `None`, a switch left unset, is Python's profile: on `OpenAI`'s own
    /// endpoint, a model it names (`gpt-4.1`, `gpt-5`, `gpt-6`, `o1`, `o3`,
    /// `o4`) takes it, any other does not.
    pub use_max_completion_tokens: Option<bool>,
    /// Whether the model takes a temperature: one that refuses it is sent
    /// none. `None`, a switch left unset, is Python's profile: on `OpenAI`'s
    /// own endpoint, a model it names (`gpt-5`, `gpt-6`, `o1`, `o3`, `o4`)
    /// takes none, any other does.
    pub supports_custom_temperature: Option<bool>,
    /// How hard a reasoning model thinks (`reasoning_effort`), when set
    /// (RFC §6.7), under the request's own.
    pub reasoning_effort: Option<String>,
    /// Headers every request carries.
    pub default_headers: Option<BTreeMap<String, String>>,
    /// Members every request's body carries, beside the provider's own.
    pub extra_body: Option<Metadata>,
    /// Whether the server honours a `json_schema` response format (RFC
    /// §6.7): `None` leaves it to the provider's default, which a server
    /// behind a base URL may not keep.
    pub supports_response_schema: Option<bool>,
    /// Whether the server takes a `json_schema` response format beside
    /// function tools (RFC §6.7): `None` leaves it on `OpenAI`'s own
    /// endpoint alone, a grammar-constrained server being unable to call a
    /// tool under the constraint.
    pub supports_response_schema_with_tools: Option<bool>,
    /// Whether the server takes `reasoning_effort` on a turn with tools,
    /// where `OpenAI`'s catalogue does not tag the model (RFC §6.7):
    /// `Some(true)` sends it as on any turn, `Some(false)` leaves it out,
    /// and `None` leaves it out with one warning per provider.
    pub supports_reasoning_effort_with_tools: Option<bool>,
    /// The assistant message field this server reads a model's earlier
    /// reasoning from, for a server whose documentation asks for it back.
    /// `None` sends no earlier reasoning (RFC §6.4): strict servers refuse
    /// a field they do not know, so it is never guessed.
    pub thinking_field: Option<String>,
    /// States that the configured model takes images (RFC §6.4), where the
    /// provider would not say so: `true` makes `supports_vision` answer
    /// yes, for a model the catalogue does not know on `OpenAI`'s own
    /// endpoint (a fine-tuned one). `false`, the default, leaves the
    /// provider's own answer, which is Python's. Python's configuration
    /// has no such member.
    pub vision: bool,
    /// Where the bearer token comes from before each request, each try of
    /// it under `max_retries` included, in place of `api_key`: a renewable
    /// credential.
    pub token_source: Option<Arc<dyn TokenSource>>,
    /// The HTTP API spoken: Chat Completions by default.
    pub api: OpenAiApi,
    /// The namespace, its name and description, under which the Responses
    /// API declares the tools.
    pub tool_namespace: Option<(String, String)>,
}

impl OpenAiConfig {
    /// The configuration calling `OpenAI`'s own endpoint with `api_key` for
    /// `model`, with Python's defaults.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            model: model.into(),
            max_tokens: Some(1024),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            include_stream_usage: true,
            use_max_completion_tokens: None,
            supports_custom_temperature: None,
            reasoning_effort: None,
            default_headers: None,
            extra_body: None,
            supports_response_schema: None,
            supports_response_schema_with_tools: None,
            supports_reasoning_effort_with_tools: None,
            thinking_field: None,
            vision: false,
            token_source: None,
            api: OpenAiApi::default(),
            tool_namespace: None,
        }
    }

    /// Whether the token bound goes as `max_completion_tokens`: the switch
    /// when set, else Python's profile of a modern model on `OpenAI`'s own
    /// endpoint (`OpenAIConfig.model_post_init`).
    pub(crate) fn takes_max_completion_tokens(&self) -> bool {
        self.use_max_completion_tokens
            .unwrap_or_else(|| self.profiled(&MAX_COMPLETION_TOKEN_MODELS))
    }

    /// Whether a temperature is sent: the switch when set, else unless
    /// Python's profile fixes the model's temperature on `OpenAI`'s own
    /// endpoint.
    pub(crate) fn takes_temperature(&self) -> bool {
        self.supports_custom_temperature
            .unwrap_or_else(|| !self.profiled(&FIXED_TEMPERATURE_MODELS))
    }

    /// Whether the model starts with one of `prefixes` on `OpenAI`'s own
    /// endpoint.
    fn profiled(&self, prefixes: &[&str]) -> bool {
        is_openai_endpoint(self.base_url.as_deref())
            && prefixes.iter().any(|prefix| self.model.starts_with(prefix))
    }
}

impl PartialEq for OpenAiConfig {
    /// Member by member; two configurations have the same source of tokens
    /// when they hold the same one, or none.
    fn eq(&self, other: &Self) -> bool {
        // Every member is named: one added later does not compile until it
        // is compared here.
        let Self {
            api_key,
            base_url,
            model,
            max_tokens,
            timeout,
            connect_timeout,
            max_retries,
            include_stream_usage,
            use_max_completion_tokens,
            supports_custom_temperature,
            reasoning_effort,
            default_headers,
            extra_body,
            supports_response_schema,
            supports_response_schema_with_tools,
            supports_reasoning_effort_with_tools,
            thinking_field,
            vision,
            token_source,
            api,
            tool_namespace,
        } = self;
        let same_source = match (token_source, &other.token_source) {
            (Some(one), Some(another)) => Arc::ptr_eq(one, another),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        };
        same_source
            && *api_key == other.api_key
            && *base_url == other.base_url
            && *model == other.model
            && *max_tokens == other.max_tokens
            && *timeout == other.timeout
            && *connect_timeout == other.connect_timeout
            && *max_retries == other.max_retries
            && *include_stream_usage == other.include_stream_usage
            && *use_max_completion_tokens == other.use_max_completion_tokens
            && *supports_custom_temperature == other.supports_custom_temperature
            && *reasoning_effort == other.reasoning_effort
            && *default_headers == other.default_headers
            && *extra_body == other.extra_body
            && *supports_response_schema == other.supports_response_schema
            && *supports_response_schema_with_tools == other.supports_response_schema_with_tools
            && *supports_reasoning_effort_with_tools == other.supports_reasoning_effort_with_tools
            && *thinking_field == other.thinking_field
            && *vision == other.vision
            && *api == other.api
            && *tool_namespace == other.tool_namespace
    }
}

impl fmt::Debug for OpenAiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.token_source.is_some() {
            "dynamic"
        } else if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        let base_url = self.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
        let headers: Option<Vec<&String>> = self
            .default_headers
            .as_ref()
            .map(|headers| headers.keys().collect());
        f.debug_struct("OpenAiConfig")
            .field("base_url", &shown_url(base_url))
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("max_retries", &self.max_retries)
            .field("api", &self.api)
            .field("vision", &self.vision)
            .field("tool_namespace", &self.tool_namespace)
            .field("thinking_field", &self.thinking_field)
            .field("reasoning_effort", &self.reasoning_effort)
            .field(
                "supports_reasoning_effort_with_tools",
                &self.supports_reasoning_effort_with_tools,
            )
            .field("default_headers", &headers)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: tests/test_providers/test_openai.py::TestOpenAIAIProvider::test_config_defaults
    // and ::test_config_profiles_old_and_custom_endpoints_conservatively
    #[test]
    fn a_modern_model_is_profiled_on_openai_s_endpoint_unless_the_switch_is_set() {
        let profile = |config: &OpenAiConfig| {
            (
                config.takes_max_completion_tokens(),
                config.takes_temperature(),
            )
        };
        let modern = OpenAiConfig::new("placeholder-not-a-key", "gpt-5.1");
        assert_eq!(modern.max_tokens, Some(1024));
        assert_eq!(modern.base_url, None);
        assert_eq!(profile(&modern), (true, false));
        let old = OpenAiConfig::new("placeholder-not-a-key", "gpt-4o");
        assert_eq!(profile(&old), (false, true));
        let between = OpenAiConfig::new("placeholder-not-a-key", "gpt-4.1");
        assert_eq!(profile(&between), (true, true));
        let custom = OpenAiConfig {
            base_url: Some("http://localhost:11434/v1".to_owned()),
            ..modern.clone()
        };
        assert_eq!(profile(&custom), (false, true));
        let explicit = OpenAiConfig {
            use_max_completion_tokens: Some(false),
            supports_custom_temperature: Some(true),
            ..modern
        };
        assert_eq!(profile(&explicit), (false, true));
    }

    #[test]
    fn the_debug_form_hides_the_key_the_url_s_credentials_and_a_header_s_value() {
        let config = OpenAiConfig {
            base_url: Some(
                "https://user:placeholder-in-the-url@llm.example:8443/placeholder-path?k=placeholder"
                    .to_owned(),
            ),
            default_headers: Some(BTreeMap::from([(
                "Authorization".to_owned(),
                "Basic placeholder-in-a-header".to_owned(),
            )])),
            ..OpenAiConfig::new("placeholder-not-a-key", "local-model")
        };
        let shown = format!("{config:?}");
        assert!(!shown.contains("placeholder"), "{shown}");
        assert!(shown.contains("https://llm.example:8443"), "{shown}");
        assert!(shown.contains("api_key: \"hidden\""), "{shown}");
        assert!(shown.contains("Authorization"), "{shown}");
        let keyless = OpenAiConfig::new("", "local-model");
        assert!(format!("{keyless:?}").contains("api_key: \"none\""));
    }
}
