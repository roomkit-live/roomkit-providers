//! OpenRouter: Python's `roomkit.providers.openrouter`, its provider over
//! OpenRouter's OpenAI-compatible Chat Completions API, and its
//! configuration.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use roomkit::model::Metadata;

use crate::vendor::{Vendor, into_openai, vendor_config_form, vendor_provider};

/// OpenRouter's endpoint and how it is called: Python's `OpenRouterConfig`,
/// its fields `OpenAIConfig`'s and `site_url`, `app_name`, built with
/// [`OpenRouterConfig::new`] and struct update.
///
/// Its `Debug` never prints the API key, nor the base URL's user
/// information, path or query, which may carry credentials.
#[derive(Clone)]
pub struct OpenRouterConfig {
    /// The key a request carries as its bearer token; empty sends none.
    pub api_key: String,
    /// OpenRouter's endpoint; another URL points at a proxy or a server of your
    /// own.
    pub base_url: String,
    /// The model asked.
    pub model: String,
    /// The token bound; `None` sends none.
    pub max_tokens: Option<u32>,
    /// The longest wait between two reads of an answer.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// How many times a failed request is tried again.
    pub max_retries: u32,
    /// Whether a streamed answer asks for its usage
    /// (`stream_options.include_usage`).
    pub include_stream_usage: bool,
    /// Whether the bound goes as `max_completion_tokens` rather than `max_tokens`.
    pub use_max_completion_tokens: bool,
    /// Whether the turn's temperature is sent.
    pub supports_custom_temperature: bool,
    /// The reasoning depth; the turn's own outranks it.
    pub reasoning_effort: Option<String>,
    /// Refused when set, as Python's config refuses it: the provider applies
    /// its vendor's own rule on a turn with tools (RFC §6.7).
    pub supports_reasoning_effort_with_tools: Option<bool>,
    /// A server of your own may read a round's reasoning in a field this names;
    /// the provider sends none back unless it names one.
    pub thinking_field: Option<String>,
    /// Whether the server honours a `json_schema` response format (RFC
    /// §6.7, Python's `supports_response_schema`): `None` leaves it to the
    /// provider's default.
    pub supports_response_schema: Option<bool>,
    /// Whether the server takes a `json_schema` response format beside
    /// function tools (RFC §6.7): `None` leaves it to the provider.
    pub supports_response_schema_with_tools: Option<bool>,
    /// The headers every request carries beside the bearer token.
    pub default_headers: Option<BTreeMap<String, String>>,
    /// Members merged into every request's body, the turn's own winning.
    pub extra_body: Option<Metadata>,
    /// Sent as `HTTP-Referer`: the site OpenRouter's app attribution names.
    pub site_url: Option<String>,
    /// Sent as `X-Title`: the app's name in OpenRouter's rankings.
    pub app_name: Option<String>,
}

impl OpenRouterConfig {
    /// Whether the configuration takes a thinking field: it sends no reasoning
    /// back unless it names a field.
    pub(crate) const TAKES_THINKING_FIELD: bool = true;
    /// Whether the configuration takes the setting of a turn with tools:
    /// the vendor's own rule decides it.
    pub(crate) const TAKES_TOOL_TURN_SETTING: bool = false;
    /// Whether the configuration refuses a key that is not ASCII: the provider
    /// refuses one when it is built.
    pub(crate) const ASCII_KEY: bool = false;
    /// The model a form that names none asks: none, one is required, the
    /// value of OpenRouter being its choice.
    pub(crate) const MODEL: Option<&'static str> = None;
    /// The vendor whose rules the provider applies.
    pub(crate) const VENDOR: Vendor = Vendor::OpenRouter;

    /// The configuration calling OpenRouter's own endpoint with `api_key` for
    /// `model`, with Python's defaults.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            model: model.into(),
            max_tokens: Some(1024),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            include_stream_usage: true,
            use_max_completion_tokens: false,
            supports_custom_temperature: true,
            reasoning_effort: None,
            supports_reasoning_effort_with_tools: None,
            thinking_field: None,
            supports_response_schema: None,
            supports_response_schema_with_tools: None,
            default_headers: None,
            extra_body: None,
            site_url: None,
            app_name: None,
        }
    }

    /// The vendor, with what of its rules the configuration states.
    #[expect(
        clippy::unused_self,
        reason = "the providers' macro asks every vendor's configuration, DeepSeek's reading its own"
    )]
    pub(crate) fn vendor(&self) -> Vendor {
        Vendor::OpenRouter
    }

    /// The OpenAI configuration the provider runs on, its default headers
    /// over the attribution headers (Python's `_merged_headers`).
    pub(crate) fn into_openai(mut self) -> crate::OpenAiConfig {
        self.default_headers = attributed(&self);
        into_openai!(self)
    }
}

impl fmt::Debug for OpenRouterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("OpenRouterConfig")
            .field("base_url", &crate::endpoint::shown_url(&self.base_url))
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("site_url", &self.site_url.is_some())
            .field("app_name", &self.app_name)
            .finish_non_exhaustive()
    }
}

// Python's `OpenRouterConfig.model_dump(mode="json")`, read and written.
vendor_config_form!(
    OpenRouterConfig,
    "an OpenRouter configuration, as a JSON object",
    site_url: Option<String>,
    app_name: Option<String>
);

/// OpenRouter's attribution headers, `HTTP-Referer` for the site and
/// `X-Title` for the app, under the configuration's default headers, which
/// win over one of the same name, however it is written.
fn attributed(config: &OpenRouterConfig) -> Option<BTreeMap<String, String>> {
    let attribution = [
        ("HTTP-Referer", &config.site_url),
        ("X-Title", &config.app_name),
    ];
    let mut headers: BTreeMap<String, String> = attribution
        .into_iter()
        .filter_map(|(name, value)| Some((name.to_owned(), value.clone()?)))
        .filter(|(_, value)| !value.is_empty())
        .collect();
    for (name, value) in config.default_headers.iter().flatten() {
        headers.retain(|kept, _| !kept.eq_ignore_ascii_case(name));
        headers.insert(name.clone(), value.clone());
    }
    (!headers.is_empty()).then_some(headers)
}

vendor_provider!(
    /// A model OpenRouter routes to (RFC §6.7): Python's
    /// `OpenRouterAIProvider(config)`. A reasoning setting goes as
    /// OpenRouter's `reasoning` object, on a turn with tools as on any other;
    /// a constrained answer is routed only to an upstream that honours every
    /// parameter; its listing is the routed models, with their names,
    /// windows and modalities.
    OpenRouter,
    OpenRouterConfig
);
