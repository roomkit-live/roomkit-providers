//! What the provider is configured with: Python's `MistralConfig`, its JSON
//! form read and written as `model_dump(mode="json")` writes it.

use std::fmt;

use roomkit_providers_client::ObjectOnly;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The model a configuration that names none asks.
pub(crate) const MODEL: &str = "mistral-large-latest";

/// How a key is written in a configuration's JSON form.
const MASK: &str = "**********";

/// Mistral's key and how a model is asked: Python's `MistralConfig`, built
/// with [`MistralConfig::new`] and struct update.
///
/// Its `Debug` never prints the API key.
#[derive(Clone)]
pub struct MistralConfig {
    /// The key a request carries as a bearer token.
    pub api_key: String,
    /// The model asked; `mistral-large-latest` by default.
    pub model: String,
    /// The token bound of an answer, which a request's own outranks.
    pub max_tokens: u32,
    /// The base URL of a server that speaks Mistral's API; `None` asks
    /// Mistral's own endpoint.
    pub server_url: Option<String>,
    /// The reasoning effort of a model that takes one (`high`, `none`),
    /// which a request's outranks; `None` leaves the model's.
    pub reasoning_effort: Option<String>,
}

impl MistralConfig {
    /// The configuration calling Mistral's endpoint with `api_key`, with
    /// Python's defaults.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: MODEL.to_owned(),
            max_tokens: 1024,
            server_url: None,
            reasoning_effort: None,
        }
    }
}

impl fmt::Debug for MistralConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "none"
        } else {
            "hidden"
        };
        f.debug_struct("MistralConfig")
            .field("api_key", &api_key)
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("server_url", &self.server_url)
            .field("reasoning_effort", &self.reasoning_effort)
            .finish()
    }
}

fn defaults() -> MistralConfig {
    MistralConfig::new("")
}

fn model() -> String {
    defaults().model
}

fn max_tokens() -> u32 {
    defaults().max_tokens
}

/// The configuration as it is read: a member Python does not have is
/// ignored, as Python ignores it.
#[derive(Deserialize)]
#[serde(expecting = "a Mistral configuration, as a JSON object")]
struct Form {
    api_key: String,
    #[serde(default = "model")]
    model: String,
    #[serde(default = "max_tokens")]
    max_tokens: u32,
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
}

impl<'de> Deserialize<'de> for MistralConfig {
    /// From a JSON object only, as Python refuses a list or a number.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let form = Form::deserialize(ObjectOnly(deserializer))?;
        Ok(Self {
            api_key: form.api_key,
            model: form.model,
            max_tokens: form.max_tokens,
            server_url: form.server_url,
            reasoning_effort: form.reasoning_effort,
        })
    }
}

/// The configuration as it is written, its key masked.
#[derive(Serialize)]
struct Written<'c> {
    api_key: &'static str,
    model: &'c str,
    max_tokens: u32,
    server_url: Option<&'c str>,
    reasoning_effort: Option<&'c str>,
}

impl Serialize for MistralConfig {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Written {
            api_key: if self.api_key.is_empty() { "" } else { MASK },
            model: &self.model,
            max_tokens: self.max_tokens,
            server_url: self.server_url.as_deref(),
            reasoning_effort: self.reasoning_effort.as_deref(),
        }
        .serialize(serializer)
    }
}
