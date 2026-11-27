//! What the provider is configured with: Python's `GeminiConfig`, its JSON
//! form read and written as `model_dump(mode="json")` writes it.

use std::fmt;
use std::time::Duration;

use roomkit_providers_client::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The model a configuration that names none asks.
pub(crate) const MODEL: &str = "gemini-3.8-flash";

/// How a key is written in a configuration's JSON form.
const MASK: &str = "**********";

/// The Developer API's key and how a model is asked: Python's
/// `GeminiConfig`, built with [`GeminiConfig::new`] and struct update.
///
/// Its `Debug` never prints the API key.
#[derive(Clone)]
pub struct GeminiConfig {
    /// The key a request carries in `x-goog-api-key`.
    pub api_key: String,
    /// The model asked; `gemini-3.8-flash` by default.
    pub model: String,
    /// The token bound of an answer, which a request's own outranks.
    pub max_tokens: u32,
    /// The thinking level of a Gemini 3 model (`minimal`, `low`, `medium`,
    /// `high`), which a request's effort outranks; `None` leaves the
    /// model's.
    pub thinking_level: Option<String>,
    /// The longest wait for the first read of an answer and between two.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
}

impl GeminiConfig {
    /// The configuration calling the Developer API with `api_key`, with
    /// Python's defaults.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: MODEL.to_owned(),
            max_tokens: 1024,
            thinking_level: None,
            timeout: Duration::from_secs(60),
            connect_timeout: Duration::from_secs(5),
        }
    }
}

impl fmt::Debug for GeminiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("GeminiConfig")
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("thinking_level", &self.thinking_level)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .finish()
    }
}

fn defaults() -> GeminiConfig {
    GeminiConfig::new("")
}
fn model() -> String {
    defaults().model
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

/// Reads a duration written in seconds, from zero up.
fn seconds<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
    let written = f64::deserialize(deserializer)?;
    Duration::try_from_secs_f64(written).map_err(|_| {
        D::Error::custom(format!(
            "a duration in seconds, from zero up, is expected: got {written}"
        ))
    })
}

/// The configuration as it is read: a member Python does not have is
/// ignored, as Python ignores it.
#[derive(Deserialize)]
#[serde(expecting = "a Gemini configuration, as a JSON object")]
struct Form {
    api_key: String,
    #[serde(default = "model")]
    model: String,
    #[serde(default = "max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    thinking_level: Option<String>,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
}

impl<'de> Deserialize<'de> for GeminiConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            api_key: form.api_key,
            model: form.model,
            max_tokens: form.max_tokens,
            thinking_level: form.thinking_level,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
        })
    }
}

/// The configuration as it is written, its key masked.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    model: &'c str,
    max_tokens: u32,
    thinking_level: Option<&'c str>,
    timeout: f64,
    connect_timeout: f64,
}

impl Serialize for GeminiConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            model: &self.model,
            max_tokens: self.max_tokens,
            thinking_level: self.thinking_level.as_deref(),
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
        }
        .serialize(serializer)
    }
}
