//! What the provider is configured with: Python's `PolarGridConfig`, its
//! JSON form read and written as `model_dump(mode="json")` writes it.

use std::fmt;
use std::time::Duration;

use roomkit_providers_client::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::catalogue;

/// The model a configuration that names none asks: the one PolarGrid serves
/// on every public edge.
pub(crate) const MODEL: &str = "qwen-3.8-27b";

/// How a key is written in a configuration's JSON form.
const MASK: &str = "**********";

/// PolarGrid's key, the edge asked and how a model is asked: Python's
/// `PolarGridConfig`, built with [`PolarGridConfig::new`] and struct
/// update.
///
/// Its `Debug` never prints the API key.
#[derive(Clone)]
pub struct PolarGridConfig {
    /// The key (`pg_…`), sent as a bearer token.
    pub api_key: String,
    /// The model asked; `qwen-3.8-27b` by default.
    pub model: String,
    /// The edge to pin, by its id (`yul-01`) or its name (`montreal`), in
    /// any case; `None` asks the autorouter for the nearest edge that
    /// serves the model.
    pub region: Option<String>,
    /// The token bound of an answer, at least 1, which a request's own
    /// outranks; `None` sends what the model's window leaves.
    pub max_tokens: Option<u32>,
    /// The nucleus sampling probability, from 0 to 1.
    pub top_p: f64,
    /// Whether qwen reasons (`enable_thinking`), which a turn outranks;
    /// `None` leaves the edge's default.
    pub thinking: Option<bool>,
    /// The longest wait for a read or a write of an answer.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// How many times a request that got no answer is tried again.
    pub max_retries: u32,
    /// Python's member, which turns on the SDK's own logging: the provider
    /// logs as it does whatever it is.
    pub debug: bool,
}

impl PolarGridConfig {
    /// The configuration calling PolarGrid with `api_key`, with Python's
    /// defaults.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: MODEL.to_owned(),
            region: None,
            max_tokens: None,
            top_p: 0.9,
            thinking: None,
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            debug: false,
        }
    }
}

impl fmt::Debug for PolarGridConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("PolarGridConfig")
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("region", &self.region)
            .field("max_tokens", &self.max_tokens)
            .field("top_p", &self.top_p)
            .field("thinking", &self.thinking)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("max_retries", &self.max_retries)
            .field("debug", &self.debug)
            .finish()
    }
}

fn defaults() -> PolarGridConfig {
    PolarGridConfig::new("")
}

fn model() -> String {
    defaults().model
}

fn top_p() -> f64 {
    defaults().top_p
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

/// Reads a region, refused when it names no edge, as Python's validator
/// refuses it: a typo would make a host nothing answers.
fn region<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let region = Option::<String>::deserialize(deserializer)?;
    match region {
        Some(region) if catalogue::region_id(&region).is_none() => Err(D::Error::custom(format!(
            "unknown PolarGrid region {region:?}"
        ))),
        region => Ok(region),
    }
}

/// Reads a token bound, refused below 1 (Python's `ge=1`).
fn at_least_one<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    let bound = Option::<u32>::deserialize(deserializer)?;
    if bound == Some(0) {
        return Err(D::Error::custom("max_tokens is at least 1"));
    }
    Ok(bound)
}

/// The configuration as it is read: a member Python does not have is
/// ignored, as Python ignores it.
#[derive(Deserialize)]
#[serde(expecting = "a PolarGrid configuration, as a JSON object")]
struct Form {
    api_key: String,
    #[serde(default = "model")]
    model: String,
    #[serde(default, deserialize_with = "region")]
    region: Option<String>,
    #[serde(default, deserialize_with = "at_least_one")]
    max_tokens: Option<u32>,
    #[serde(default = "top_p")]
    top_p: f64,
    #[serde(default)]
    thinking: Option<bool>,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: u32,
    #[serde(default)]
    debug: bool,
}

impl<'de> Deserialize<'de> for PolarGridConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            api_key: form.api_key,
            model: form.model,
            region: form.region,
            max_tokens: form.max_tokens,
            top_p: form.top_p,
            thinking: form.thinking,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            debug: form.debug,
        })
    }
}

/// The configuration as it is written, its key masked.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    model: &'c str,
    region: Option<&'c str>,
    max_tokens: Option<u32>,
    top_p: f64,
    thinking: Option<bool>,
    timeout: f64,
    connect_timeout: f64,
    max_retries: u32,
    debug: bool,
}

impl Serialize for PolarGridConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            model: &self.model,
            region: self.region.as_deref(),
            max_tokens: self.max_tokens,
            top_p: self.top_p,
            thinking: self.thinking,
            timeout: self.timeout.as_secs_f64(),
            connect_timeout: self.connect_timeout.as_secs_f64(),
            max_retries: self.max_retries,
            debug: self.debug,
        }
        .serialize(serializer)
    }
}
