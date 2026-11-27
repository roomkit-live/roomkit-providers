//! How a configuration is read and written: the JSON form of Python's
//! `OpenAIConfig`, read from an object only, a member left out taking its
//! default.

use std::collections::BTreeMap;
use std::time::Duration;

use roomkit::model::Metadata;
use roomkit_providers_client::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::OpenAiConfig;

/// What Python writes in place of a key (`SecretStr`).
pub(crate) const MASK: &str = "**********";

/// [`OpenAiConfig`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "an OpenAI configuration, as a JSON object")]
struct ConfigForm {
    api_key: String,
    #[serde(default)]
    base_url: Option<String>,
    model: String,
    #[serde(default = "default_max_tokens")]
    max_tokens: Option<u32>,
    #[serde(default = "default_timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "default_connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: u32,
    #[serde(default = "default_include_stream_usage")]
    include_stream_usage: bool,
    #[serde(default, deserialize_with = "stated")]
    use_max_completion_tokens: Option<bool>,
    #[serde(default, deserialize_with = "stated")]
    supports_custom_temperature: Option<bool>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    default_headers: Option<BTreeMap<String, String>>,
    #[serde(default)]
    extra_body: Option<Metadata>,
    #[serde(default)]
    supports_response_schema: Option<bool>,
    #[serde(default)]
    supports_response_schema_with_tools: Option<bool>,
    #[serde(default)]
    supports_reasoning_effort_with_tools: Option<bool>,
    #[serde(default)]
    thinking_field: Option<String>,
}

/// The defaults of the members a form may leave out: those of
/// [`OpenAiConfig::new`].
fn defaults() -> OpenAiConfig {
    OpenAiConfig::new("", "")
}

fn default_max_tokens() -> Option<u32> {
    defaults().max_tokens
}

fn default_timeout() -> Duration {
    defaults().timeout
}

fn default_connect_timeout() -> Duration {
    defaults().connect_timeout
}

fn default_include_stream_usage() -> bool {
    defaults().include_stream_usage
}

/// Reads a duration written in seconds, as Python's `float` holds one.
pub(crate) fn seconds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let written = f64::deserialize(deserializer)?;
    Duration::try_from_secs_f64(written).map_err(|_| {
        D::Error::custom(format!(
            "a duration in seconds, from zero up, is expected: got {written}"
        ))
    })
}

/// Reads a switch that is stated: `true` or `false`, never `null`, which
/// Python refuses for it. Only a member left out leaves the switch unset.
fn stated<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<bool>, D::Error> {
    bool::deserialize(deserializer).map(Some)
}

impl From<ConfigForm> for OpenAiConfig {
    fn from(form: ConfigForm) -> Self {
        Self {
            api_key: form.api_key,
            base_url: form.base_url,
            model: form.model,
            max_tokens: form.max_tokens,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            include_stream_usage: form.include_stream_usage,
            use_max_completion_tokens: form.use_max_completion_tokens,
            supports_custom_temperature: form.supports_custom_temperature,
            reasoning_effort: form.reasoning_effort,
            default_headers: form.default_headers,
            extra_body: form.extra_body,
            supports_response_schema: form.supports_response_schema,
            supports_response_schema_with_tools: form.supports_response_schema_with_tools,
            supports_reasoning_effort_with_tools: form.supports_reasoning_effort_with_tools,
            thinking_field: form.thinking_field,
            vision: false,
            token_source: None,
            api: crate::OpenAiApi::default(),
            tool_namespace: None,
        }
    }
}

impl<'de> Deserialize<'de> for OpenAiConfig {
    /// From a JSON object only: a list or a number is refused, as Python
    /// refuses both.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ConfigForm::deserialize(ObjectOnly(deserializer)).map(Self::from)
    }
}

/// [`OpenAiConfig`] as it is written: Python's members, in Python's order.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    base_url: Option<&'c str>,
    model: &'c str,
    max_tokens: Option<u32>,
    timeout: f64,
    connect_timeout: f64,
    max_retries: u32,
    include_stream_usage: bool,
    use_max_completion_tokens: bool,
    supports_custom_temperature: bool,
    reasoning_effort: Option<&'c str>,
    default_headers: Option<&'c BTreeMap<String, String>>,
    extra_body: Option<&'c Metadata>,
    supports_response_schema: Option<bool>,
    supports_response_schema_with_tools: Option<bool>,
    supports_reasoning_effort_with_tools: Option<bool>,
    thinking_field: Option<&'c str>,
}

impl Serialize for OpenAiConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            base_url: self.base_url.as_deref(),
            model: &self.model,
            max_tokens: self.max_tokens,
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            max_retries: self.max_retries,
            include_stream_usage: self.include_stream_usage,
            use_max_completion_tokens: self.takes_max_completion_tokens(),
            supports_custom_temperature: self.takes_temperature(),
            reasoning_effort: self.reasoning_effort.as_deref(),
            default_headers: self.default_headers.as_ref(),
            extra_body: self.extra_body.as_ref(),
            supports_response_schema: self.supports_response_schema,
            supports_response_schema_with_tools: self.supports_response_schema_with_tools,
            supports_reasoning_effort_with_tools: self.supports_reasoning_effort_with_tools,
            thinking_field: self.thinking_field.as_deref(),
        }
        .serialize(serializer)
    }
}

/// Reads a member a vendor's configuration leaves unset: `null` only, as
/// Python's derivative configuration refuses any other value, the vendor's
/// own rule making the member moot (RFC §6.4, §6.7).
pub(crate) fn left_null<'de, D: Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    match Option::<serde::de::IgnoredAny>::deserialize(deserializer)? {
        None => Ok(()),
        Some(_) => Err(D::Error::custom(
            "the vendor's own rule decides this member: it is left unset",
        )),
    }
}
