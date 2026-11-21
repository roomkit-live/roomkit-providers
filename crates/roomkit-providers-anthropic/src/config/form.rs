//! How a configuration is read and written: the JSON form of Python's
//! `AnthropicConfig`, read from an object only, a member left out taking
//! its default.

use std::collections::BTreeMap;
use std::time::Duration;

use roomkit_providers_llm::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::AnthropicConfig;

/// What Python writes in place of a key (`SecretStr`).
const MASK: &str = "**********";

/// [`AnthropicConfig`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "an Anthropic configuration, as a JSON object")]
struct ConfigForm {
    api_key: String,
    model: String,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    #[serde(default = "default_timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "default_connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: u32,
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    extra_headers: Option<BTreeMap<String, String>>,
    #[serde(default = "default_enable_prompt_caching")]
    enable_prompt_caching: bool,
    #[serde(default, deserialize_with = "stated")]
    use_adaptive_thinking: Option<bool>,
    #[serde(default, deserialize_with = "stated")]
    supports_custom_temperature: Option<bool>,
}

/// The defaults of the members a form may leave out: those of
/// [`AnthropicConfig::new`].
fn defaults() -> AnthropicConfig {
    AnthropicConfig::new("", "")
}

fn default_max_tokens() -> u32 {
    defaults().max_tokens
}

fn default_timeout() -> Duration {
    defaults().timeout
}

fn default_connect_timeout() -> Duration {
    defaults().connect_timeout
}

fn default_enable_prompt_caching() -> bool {
    defaults().enable_prompt_caching
}

/// Reads a duration written in seconds, as Python's `float` holds one.
fn seconds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
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

impl From<ConfigForm> for AnthropicConfig {
    fn from(form: ConfigForm) -> Self {
        Self {
            api_key: form.api_key,
            model: form.model,
            max_tokens: form.max_tokens,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            base_url: form.base_url,
            extra_headers: form.extra_headers,
            enable_prompt_caching: form.enable_prompt_caching,
            use_adaptive_thinking: form.use_adaptive_thinking,
            supports_custom_temperature: form.supports_custom_temperature,
        }
    }
}

impl<'de> Deserialize<'de> for AnthropicConfig {
    /// From a JSON object only: a list or a number is refused, as Python
    /// refuses both.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ConfigForm::deserialize(ObjectOnly(deserializer)).map(Self::from)
    }
}

/// [`AnthropicConfig`] as it is written: Python's members, in Python's
/// order.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    model: &'c str,
    max_tokens: u32,
    timeout: f64,
    connect_timeout: f64,
    max_retries: u32,
    base_url: Option<String>,
    extra_headers: Option<&'c BTreeMap<String, String>>,
    enable_prompt_caching: bool,
    use_adaptive_thinking: bool,
    supports_custom_temperature: bool,
}

impl Serialize for AnthropicConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            model: &self.model,
            max_tokens: self.max_tokens,
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            max_retries: self.max_retries,
            base_url: self.kept_base_url(),
            extra_headers: self.extra_headers.as_ref(),
            enable_prompt_caching: self.enable_prompt_caching,
            use_adaptive_thinking: self.thinks_adaptively(),
            supports_custom_temperature: self.takes_temperature(),
        }
        .serialize(serializer)
    }
}
