//! vLLM: Python's `roomkit.providers.vllm`, its provider over a vLLM
//! server's OpenAI-compatible Chat Completions API (Python's
//! `create_vllm_provider`), and its configuration.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use roomkit::model::Metadata;
use roomkit_ai::ProviderError;
use roomkit_providers_client::ObjectOnly;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::config::form::{MASK, seconds};
use crate::vendor::{Vendor, vendor_provider};

/// The member of a body that carries a chat template's switches.
const TEMPLATE: &str = "chat_template_kwargs";

/// A vLLM server and how it is called: Python's `VLLMConfig`, built with
/// [`VllmConfig::new`] and struct update. It is no `OpenAIConfig`: a key
/// that defaults to `none`, `headers` in place of default headers, vLLM's
/// sampling members and its chat template's switches.
///
/// Its `Debug` never prints the API key, nor the base URL's user
/// information, path or query, which may carry credentials.
#[derive(Clone)]
pub struct VllmConfig {
    /// The model the server loaded.
    pub model: String,
    /// The server's OpenAI-compatible endpoint.
    pub base_url: String,
    /// The bearer token, `vllm serve --api-key`'s: `none` by default, for a
    /// local server that asks for none.
    pub api_key: String,
    /// The token bound.
    pub max_tokens: u32,
    /// The longest wait between two reads of an answer.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// How many times a failed request is tried again.
    pub max_retries: u32,
    /// Whether a streamed answer asks for its usage
    /// (`stream_options.include_usage`).
    pub include_stream_usage: bool,
    /// The headers every request carries, for a reverse proxy or another
    /// `Authorization` scheme.
    pub headers: Option<BTreeMap<String, String>>,
    /// Nucleus sampling's cutoff; `None` leaves the server's.
    pub top_p: Option<f64>,
    /// Top-k sampling's cutoff, a vLLM member; `None` leaves the server's.
    pub top_k: Option<i64>,
    /// The least probability a token is sampled at, a vLLM member.
    pub min_p: Option<f64>,
    /// The penalty on tokens the answer already holds.
    pub presence_penalty: Option<f64>,
    /// vLLM's multiplicative penalty on repetition.
    pub repetition_penalty: Option<f64>,
    /// Members merged into every request's body, over the sampling members
    /// above; its `chat_template_kwargs` over the template's switches below.
    pub extra_body: Option<Metadata>,
    /// The chat template's thinking switch; `None` leaves the model's.
    pub enable_thinking: Option<bool>,
    /// The chat template's reasoning effort; `None` leaves the model's.
    pub reasoning_effort: Option<String>,
    /// Whether the server honours a `json_schema` response format (RFC
    /// §6.7): `None` leaves it to the provider's default, which sends it.
    pub supports_response_schema: Option<bool>,
    /// Whether the server takes a `json_schema` response format beside
    /// function tools (RFC §6.7): `None` leaves it out, guided decoding
    /// keeping the model from calling a tool under it.
    pub supports_response_schema_with_tools: Option<bool>,
}

impl VllmConfig {
    /// The vendor whose rules the provider applies.
    pub(crate) const VENDOR: Vendor = Vendor::Vllm;

    /// The configuration calling the local server for `model`, with
    /// Python's defaults.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            base_url: "http://localhost:8000/v1".to_owned(),
            api_key: "none".to_owned(),
            max_tokens: 1024,
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            include_stream_usage: true,
            headers: None,
            top_p: None,
            top_k: None,
            min_p: None,
            presence_penalty: None,
            repetition_penalty: None,
            extra_body: None,
            enable_thinking: None,
            reasoning_effort: None,
            supports_response_schema: None,
            supports_response_schema_with_tools: None,
        }
    }

    /// The body every request carries beside `OpenAI`'s members (Python's
    /// `_openai_config`): the sampling members stated, the extra body over
    /// them, and the template's switches the configuration states under the
    /// extra body's own.
    fn body(&self) -> Option<Metadata> {
        let sampling = [
            ("top_p", self.top_p.map(Value::from)),
            ("top_k", self.top_k.map(Value::from)),
            ("min_p", self.min_p.map(Value::from)),
            ("presence_penalty", self.presence_penalty.map(Value::from)),
            (
                "repetition_penalty",
                self.repetition_penalty.map(Value::from),
            ),
        ];
        let explicit = self.extra_body.clone().unwrap_or_default();
        let mut body: Metadata = sampling
            .into_iter()
            .filter_map(|(name, value)| Some((name.to_owned(), value?)))
            .collect();
        body.extend(explicit.clone());
        let mut template = serde_json::Map::new();
        if let Some(enabled) = self.enable_thinking {
            template.insert("enable_thinking".to_owned(), enabled.into());
        }
        if let Some(effort) = &self.reasoning_effort {
            template.insert("reasoning_effort".to_owned(), effort.as_str().into());
        }
        if !template.is_empty() {
            let stated = explicit.get(TEMPLATE).and_then(Value::as_object);
            template.extend(stated.cloned().unwrap_or_default());
            body.insert(TEMPLATE.to_owned(), Value::Object(template));
        }
        (!body.is_empty()).then_some(body)
    }

    /// The provider the configuration builds: `OpenAI`'s wire at the
    /// server, the configuration's effort riding the chat template, never
    /// `reasoning_effort`.
    pub(crate) fn provider(self) -> Result<crate::OpenAi, ProviderError> {
        let extra_body = self.body();
        let openai = crate::OpenAiConfig {
            base_url: Some(self.base_url),
            max_tokens: Some(self.max_tokens),
            timeout: self.timeout,
            connect_timeout: self.connect_timeout,
            max_retries: self.max_retries,
            include_stream_usage: self.include_stream_usage,
            default_headers: self.headers,
            extra_body,
            supports_response_schema: self.supports_response_schema,
            supports_response_schema_with_tools: self.supports_response_schema_with_tools,
            ..crate::OpenAiConfig::new(self.api_key, self.model)
        };
        crate::OpenAi::for_vendor(openai, Self::VENDOR)
    }
}

impl fmt::Debug for VllmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("VllmConfig")
            .field("base_url", &crate::endpoint::shown_url(&self.base_url))
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("enable_thinking", &self.enable_thinking)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish_non_exhaustive()
    }
}

/// The chat template's switches of a turn (Python's
/// `_VLLMProvider._resolve_template_kwargs`), over those `extra_body`, the
/// body the configuration states, carries: the turn's switch, its budget
/// first, and its effort unless `none`, on a turn with tools as on any
/// other. Nothing when neither states one.
pub(crate) fn template_of_the_turn(
    request: &roomkit_ai::AiContext,
    extra_body: Option<&Metadata>,
) -> Option<Value> {
    let configured = extra_body.and_then(|body| body.get(TEMPLATE));
    let mut resolved = configured
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(enabled) = crate::reasoning::thinking_switch(request) {
        resolved.insert("enable_thinking".to_owned(), enabled.into());
    }
    let effort = request.reasoning_effort.as_deref();
    if let Some(effort) = effort.filter(|effort| *effort != "none") {
        resolved.insert("reasoning_effort".to_owned(), effort.into());
    }
    (!resolved.is_empty()).then(|| Value::Object(resolved))
}

// Python's `VLLMConfig.model_dump(mode="json")`, read and written: its
// members in its order, the key written masked, a member Python does not
// have ignored.

fn defaults() -> VllmConfig {
    VllmConfig::new("")
}
fn base_url() -> String {
    defaults().base_url
}
fn api_key() -> String {
    defaults().api_key
}
fn max_tokens() -> u32 {
    defaults().max_tokens
}
fn timeout() -> Duration {
    defaults().timeout
}
fn connect_timeout() -> Duration {
    defaults().connect_timeout
}
fn include_stream_usage() -> bool {
    defaults().include_stream_usage
}

/// The configuration as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a vLLM configuration, as a JSON object")]
struct Form {
    model: String,
    #[serde(default = "base_url")]
    base_url: String,
    #[serde(default = "api_key")]
    api_key: String,
    #[serde(default = "max_tokens")]
    max_tokens: u32,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: u32,
    #[serde(default = "include_stream_usage")]
    include_stream_usage: bool,
    #[serde(default)]
    headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    top_k: Option<i64>,
    #[serde(default)]
    min_p: Option<f64>,
    #[serde(default)]
    presence_penalty: Option<f64>,
    #[serde(default)]
    repetition_penalty: Option<f64>,
    #[serde(default)]
    extra_body: Option<Metadata>,
    #[serde(default)]
    enable_thinking: Option<bool>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    supports_response_schema: Option<bool>,
    #[serde(default)]
    supports_response_schema_with_tools: Option<bool>,
}

impl<'de> Deserialize<'de> for VllmConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            model: form.model,
            base_url: form.base_url,
            api_key: form.api_key,
            max_tokens: form.max_tokens,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            include_stream_usage: form.include_stream_usage,
            headers: form.headers,
            top_p: form.top_p,
            top_k: form.top_k,
            min_p: form.min_p,
            presence_penalty: form.presence_penalty,
            repetition_penalty: form.repetition_penalty,
            extra_body: form.extra_body,
            enable_thinking: form.enable_thinking,
            reasoning_effort: form.reasoning_effort,
            supports_response_schema: form.supports_response_schema,
            supports_response_schema_with_tools: form.supports_response_schema_with_tools,
        })
    }
}

/// The configuration as it is written.
#[derive(Serialize)]
struct Written<'c> {
    model: &'c str,
    base_url: &'c str,
    api_key: &'static str,
    max_tokens: u32,
    timeout: f64,
    connect_timeout: f64,
    max_retries: u32,
    include_stream_usage: bool,
    headers: Option<&'c BTreeMap<String, String>>,
    top_p: Option<f64>,
    top_k: Option<i64>,
    min_p: Option<f64>,
    presence_penalty: Option<f64>,
    repetition_penalty: Option<f64>,
    extra_body: Option<&'c Metadata>,
    enable_thinking: Option<bool>,
    reasoning_effort: Option<&'c str>,
    supports_response_schema: Option<bool>,
    supports_response_schema_with_tools: Option<bool>,
}

impl Serialize for VllmConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            model: &self.model,
            base_url: &self.base_url,
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            max_tokens: self.max_tokens,
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            max_retries: self.max_retries,
            include_stream_usage: self.include_stream_usage,
            headers: self.headers.as_ref(),
            top_p: self.top_p,
            top_k: self.top_k,
            min_p: self.min_p,
            presence_penalty: self.presence_penalty,
            repetition_penalty: self.repetition_penalty,
            extra_body: self.extra_body.as_ref(),
            enable_thinking: self.enable_thinking,
            reasoning_effort: self.reasoning_effort.as_deref(),
            supports_response_schema: self.supports_response_schema,
            supports_response_schema_with_tools: self.supports_response_schema_with_tools,
        }
        .serialize(serializer)
    }
}

vendor_provider!(
    /// A model a vLLM server loaded (RFC §6.7): what Python's
    /// `create_vllm_provider(config)` builds. A reasoning setting rides the
    /// chat template's `chat_template_kwargs`, the turn's over the
    /// configuration's, on a turn with tools as on any other, never a
    /// top-level `reasoning_effort`; the sampling members ride the body;
    /// whether it reads images is the server's to answer.
    Vllm,
    VllmConfig,
    built by its configuration
);
