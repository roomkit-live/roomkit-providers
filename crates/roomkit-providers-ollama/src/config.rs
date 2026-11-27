//! What the provider is configured with: Python's `OllamaConfig`, its JSON
//! form read and written as `model_dump(mode="json")` writes it.
//!
//! Two of Python's members hold one of two kinds, which Rust names: `think`
//! a switch or a level ([`Think`], Python's `bool | ThinkEffort`), and
//! `keep_alive` a duration or a count of seconds ([`KeepAlive`], Python's
//! `str | int`).

use std::fmt;
use std::time::Duration;

use roomkit_providers_client::ObjectOnly;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The host a configuration that names none asks: the local daemon.
pub(crate) const HOST: &str = "http://localhost:11434";

/// The model a configuration that names none asks.
pub(crate) const MODEL: &str = "llama3.2";

/// How a key is written in a configuration's JSON form.
const MASK: &str = "**********";

/// A reasoning level of a model that takes one (gpt-oss): Python's
/// `ThinkEffort`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThinkEffort {
    /// `low`.
    Low,
    /// `medium`.
    Medium,
    /// `high`.
    High,
}

impl ThinkEffort {
    /// The level as Ollama names it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn named(name: &str) -> Option<Self> {
        match name {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// Whether and how hard the model reasons: Python's `think`, a switch or a
/// level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Think {
    /// Thinking on or off.
    Switch(bool),
    /// A level, for a model that takes one.
    Effort(ThinkEffort),
}

/// How long the model stays loaded after a request: Python's `keep_alive`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeepAlive {
    /// A duration as Ollama reads one (`"5m"`, `"30s"`).
    Duration(String),
    /// A count of seconds: `-1` keeps the model loaded, `0` unloads it.
    Seconds(i64),
}

/// An Ollama server and how a model is asked of it: Python's
/// `OllamaConfig`, built with [`OllamaConfig::default`] and struct update.
///
/// Its `Debug` never prints the API key.
#[derive(Clone)]
pub struct OllamaConfig {
    /// The server: a URL, or a host and a port, as the ollama SDK reads it.
    pub host: String,
    /// The model asked; `llama3.2` by default.
    pub model: String,
    /// The token bound of an answer (`num_predict`), which a request's own
    /// outranks; `None` leaves the server's.
    pub max_tokens: Option<i64>,
    /// The longest wait for a read or a write of an answer.
    pub timeout: Duration,
    /// The longest wait for a connection.
    pub connect_timeout: Duration,
    /// Python's member, which nothing reads: the provider tries no request
    /// again, as the SDK tries none.
    pub max_retries: i64,
    /// Whether and how hard the model reasons; `None` leaves the model's.
    pub think: Option<Think>,
    /// How long the model stays loaded; `None` leaves the server's.
    pub keep_alive: Option<KeepAlive>,
    /// The context window (`options.num_ctx`).
    pub num_ctx: Option<i64>,
    /// The nucleus sampling cutoff (`options.top_p`).
    pub top_p: Option<f64>,
    /// The top-k cutoff (`options.top_k`).
    pub top_k: Option<i64>,
    /// The minimum probability cutoff (`options.min_p`).
    pub min_p: Option<f64>,
    /// A bearer token, sent as `Authorization` over a configured one.
    pub api_key: Option<String>,
    /// Headers sent with every request, in their order.
    pub headers: Option<Vec<(String, String)>>,
}

impl Default for OllamaConfig {
    /// Python's defaults: the local daemon, `llama3.2`.
    fn default() -> Self {
        Self {
            host: HOST.to_owned(),
            model: MODEL.to_owned(),
            max_tokens: None,
            timeout: Duration::from_secs(120),
            connect_timeout: Duration::from_secs(5),
            max_retries: 0,
            think: None,
            keep_alive: None,
            num_ctx: None,
            top_p: None,
            top_k: None,
            min_p: None,
            api_key: None,
            headers: None,
        }
    }
}

impl fmt::Debug for OllamaConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = self.api_key.as_ref().map(|_| "hidden");
        let headers = self.headers.as_ref().map(|headers| {
            headers
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
        });
        f.debug_struct("OllamaConfig")
            .field("host", &self.host)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("think", &self.think)
            .field("keep_alive", &self.keep_alive)
            .field("num_ctx", &self.num_ctx)
            .field("top_p", &self.top_p)
            .field("top_k", &self.top_k)
            .field("min_p", &self.min_p)
            .field("api_key", &api_key)
            .field("headers", &headers)
            .finish_non_exhaustive()
    }
}

fn defaults() -> OllamaConfig {
    OllamaConfig::default()
}

fn host() -> String {
    defaults().host
}

fn model() -> String {
    defaults().model
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

/// Reads `think`: a switch, or one of the three levels.
fn think<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Think>, D::Error> {
    let refused = || D::Error::custom("think is a boolean or one of low, medium and high");
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(on)) => Ok(Some(Think::Switch(on))),
        Some(Value::String(level)) => ThinkEffort::named(&level)
            .map(|level| Some(Think::Effort(level)))
            .ok_or_else(refused),
        Some(_) => Err(refused()),
    }
}

/// Reads `keep_alive` as Python's validator does: a text that holds a whole
/// number, white space around it, is that count of seconds, as Ollama reads
/// a text as a duration that needs a unit.
fn keep_alive<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<KeepAlive>, D::Error> {
    let refused = || D::Error::custom("keep_alive is a text or a whole number of seconds");
    match Option::<Value>::deserialize(deserializer)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(match text.trim().parse::<i64>() {
            Ok(seconds) => KeepAlive::Seconds(seconds),
            Err(_) => KeepAlive::Duration(text),
        })),
        Some(Value::Number(number)) => number
            .as_i64()
            .map(|seconds| Some(KeepAlive::Seconds(seconds)))
            .ok_or_else(refused),
        Some(_) => Err(refused()),
    }
}

/// Reads `headers`, in their order.
fn headers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<(String, String)>>, D::Error> {
    let Some(headers) = Option::<serde_json::Map<String, Value>>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let read = headers.into_iter().map(|(name, value)| {
        let Value::String(value) = value else {
            return Err(D::Error::custom("a header's value is a text"));
        };
        Ok((name, value))
    });
    read.collect::<Result<_, _>>().map(Some)
}

/// The configuration as it is read: a member Python does not have is
/// ignored, as Python ignores it.
#[derive(Deserialize)]
#[serde(expecting = "an Ollama configuration, as a JSON object")]
struct Form {
    #[serde(default = "host")]
    host: String,
    #[serde(default = "model")]
    model: String,
    #[serde(default)]
    max_tokens: Option<i64>,
    #[serde(default = "timeout", deserialize_with = "seconds")]
    timeout: Duration,
    #[serde(default = "connect_timeout", deserialize_with = "seconds")]
    connect_timeout: Duration,
    #[serde(default)]
    max_retries: i64,
    #[serde(default, deserialize_with = "think")]
    think: Option<Think>,
    #[serde(default, deserialize_with = "keep_alive")]
    keep_alive: Option<KeepAlive>,
    #[serde(default)]
    num_ctx: Option<i64>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    top_k: Option<i64>,
    #[serde(default)]
    min_p: Option<f64>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default, deserialize_with = "headers")]
    headers: Option<Vec<(String, String)>>,
}

impl<'de> Deserialize<'de> for OllamaConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            host: form.host,
            model: form.model,
            max_tokens: form.max_tokens,
            timeout: form.timeout,
            connect_timeout: form.connect_timeout,
            max_retries: form.max_retries,
            think: form.think,
            keep_alive: form.keep_alive,
            num_ctx: form.num_ctx,
            top_p: form.top_p,
            top_k: form.top_k,
            min_p: form.min_p,
            api_key: form.api_key,
            headers: form.headers,
        })
    }
}

/// `think` as Python writes it.
pub(crate) fn think_value(think: Think) -> Value {
    match think {
        Think::Switch(on) => Value::Bool(on),
        Think::Effort(level) => Value::String(level.as_str().to_owned()),
    }
}

/// `keep_alive` as Python's configuration writes it.
pub(crate) fn keep_alive_value(keep_alive: &KeepAlive) -> Value {
    match keep_alive {
        KeepAlive::Duration(text) => Value::String(text.clone()),
        KeepAlive::Seconds(seconds) => Value::from(*seconds),
    }
}

impl Serialize for OllamaConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut written = serde_json::Map::new();
        written.insert("host".into(), self.host.clone().into());
        written.insert("model".into(), self.model.clone().into());
        written.insert("max_tokens".into(), self.max_tokens.into());
        written.insert("timeout".into(), self.timeout.as_secs_f64().into());
        let connect_timeout = self.connect_timeout.as_secs_f64();
        written.insert("connect_timeout".into(), connect_timeout.into());
        written.insert("max_retries".into(), self.max_retries.into());
        written.insert("think".into(), self.think.map_or(Value::Null, think_value));
        let keep_alive = self
            .keep_alive
            .as_ref()
            .map_or(Value::Null, keep_alive_value);
        written.insert("keep_alive".into(), keep_alive);
        written.insert("num_ctx".into(), self.num_ctx.into());
        written.insert("top_p".into(), self.top_p.into());
        written.insert("top_k".into(), self.top_k.into());
        written.insert("min_p".into(), self.min_p.into());
        let api_key = self
            .api_key
            .as_deref()
            .map(|key| if key.is_empty() { "" } else { MASK });
        written.insert("api_key".into(), api_key.into());
        let headers = self.headers.as_ref().map(|headers| {
            let pairs = headers
                .iter()
                .map(|(name, value)| (name.clone(), value.clone().into()));
            Value::Object(pairs.collect())
        });
        written.insert("headers".into(), headers.unwrap_or(Value::Null));
        Value::Object(written).serialize(serializer)
    }
}
