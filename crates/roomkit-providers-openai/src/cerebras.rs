//! Cerebras: Python's `roomkit.providers.cerebras`, its provider over Cerebras's
//! OpenAI-compatible Chat Completions API, and its configuration.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use roomkit::model::Metadata;
use serde::{Deserialize, Serialize};

use crate::vendor::{Vendor, into_openai, vendor_config_form, vendor_provider};

/// How Cerebras's answer carries a model's reasoning: Python's
/// `CerebrasConfig.reasoning_format`, written as Python writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CerebrasReasoningFormat {
    /// Apart from the text, which the provider reads as reasoning.
    Parsed,
    /// In the text, as the model wrote it.
    Raw,
    /// Not at all.
    Hidden,
    /// As the model decides: `none`, a format of its own.
    None,
}

impl CerebrasReasoningFormat {
    /// The value the request carries.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "parsed",
            Self::Raw => "raw",
            Self::Hidden => "hidden",
            Self::None => "none",
        }
    }
}

/// Cerebras's endpoint and how it is called: Python's `CerebrasConfig`, its
/// fields `OpenAIConfig`'s and `reasoning_format`, `clear_thinking`, built with
/// [`CerebrasConfig::new`] and struct update.
///
/// Its `Debug` never prints the API key, nor the base URL's user
/// information, path or query, which may carry credentials.
#[derive(Clone)]
pub struct CerebrasConfig {
    /// The key a request carries as its bearer token; empty sends none.
    pub api_key: String,
    /// Cerebras's endpoint; another URL points at a proxy or a server of your
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
    /// Refused when set, as Python's config refuses it: the provider sends
    /// reasoning where its vendor reads it (RFC §6.4).
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
    /// How the answer carries the model's reasoning; `Parsed`, the default,
    /// keeps it apart from the text. `None` sends no `reasoning_format`.
    pub reasoning_format: Option<CerebrasReasoningFormat>,
    /// Whether the server drops earlier reasoning before it prompts the model;
    /// `None` leaves the server's default.
    pub clear_thinking: Option<bool>,
}

impl CerebrasConfig {
    /// Whether the configuration takes a thinking field: the provider sends
    /// reasoning where Cerebras reads it.
    pub(crate) const TAKES_THINKING_FIELD: bool = false;
    /// Whether the configuration takes the setting of a turn with tools:
    /// the vendor's own rule decides it.
    pub(crate) const TAKES_TOOL_TURN_SETTING: bool = false;
    /// Whether the configuration refuses a key that is not ASCII: the provider
    /// refuses one when it is built.
    pub(crate) const ASCII_KEY: bool = false;
    /// The model a form that names none asks: none, one is required.
    pub(crate) const MODEL: Option<&'static str> = None;
    /// The vendor whose rules the provider applies.
    pub(crate) const VENDOR: Vendor = Vendor::Cerebras {
        reasoning_format: Some("parsed"),
        clear_thinking: None,
    };

    /// The configuration calling Cerebras's own endpoint with `api_key` for
    /// `model`, with Python's defaults.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.cerebras.ai/v1".to_owned(),
            model: model.into(),
            max_tokens: Some(1024),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            include_stream_usage: false,
            use_max_completion_tokens: true,
            supports_custom_temperature: true,
            reasoning_effort: None,
            supports_reasoning_effort_with_tools: None,
            thinking_field: None,
            supports_response_schema: None,
            supports_response_schema_with_tools: None,
            default_headers: None,
            extra_body: None,
            reasoning_format: Some(CerebrasReasoningFormat::Parsed),
            clear_thinking: None,
        }
    }

    /// The vendor, with what of its rules the configuration states.
    pub(crate) fn vendor(&self) -> Vendor {
        Vendor::Cerebras {
            reasoning_format: self.reasoning_format.map(CerebrasReasoningFormat::as_str),
            clear_thinking: self.clear_thinking,
        }
    }

    /// The OpenAI configuration the provider runs on.
    pub(crate) fn into_openai(self) -> crate::OpenAiConfig {
        into_openai!(self)
    }
}

impl fmt::Debug for CerebrasConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("CerebrasConfig")
            .field("base_url", &crate::endpoint::shown_url(&self.base_url))
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("reasoning_format", &self.reasoning_format)
            .field("clear_thinking", &self.clear_thinking)
            .finish_non_exhaustive()
    }
}

// Python's `CerebrasConfig.model_dump(mode="json")`, read and written.
vendor_config_form!(CerebrasConfig, "a Cerebras configuration, as a JSON object", reasoning_format: Option<CerebrasReasoningFormat>, clear_thinking: Option<bool>);

vendor_provider!(
    /// A Cerebras model (RFC §6.7): Python's `CerebrasAIProvider(config)`. A
    /// round's reasoning goes back in `reasoning`, its effort floored by the
    /// model's catalogue entry on a turn with tools too, and an array the
    /// model wrote as the JSON text of the array is read back where the
    /// tool declares one.
    Cerebras,
    CerebrasConfig
);
