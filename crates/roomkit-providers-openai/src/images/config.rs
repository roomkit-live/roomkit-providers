//! What an OpenAI image provider is configured with: Python's
//! `OpenAIImageConfig`, and its JSON form.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::config::form::seconds;
use crate::endpoint::shown_url;

/// The configuration of an [`OpenAiImage`](super::OpenAiImage): Python's
/// `OpenAIImageConfig`. Apart from the conversational configuration, as
/// Python keeps it: it configures another endpoint, of a lineup of models
/// disjoint from the conversational one (RFC §25.6).
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(
    deny_unknown_fields,
    expecting = "an OpenAI image configuration, as a JSON object"
)]
pub struct OpenAiImageConfig {
    /// The key every request carries as its bearer token.
    pub api_key: String,
    /// The base URL of an endpoint that speaks the images API: `OpenAI`'s
    /// own when `None`.
    #[serde(default)]
    pub base_url: Option<String>,
    /// The image model (`gpt-image-2`).
    pub model: String,
    /// The quality every request asks unless it asks its own: the model's
    /// default when `None`.
    #[serde(default)]
    pub quality: Option<String>,
    /// The background every request asks unless it asks its own.
    #[serde(default)]
    pub background: Option<String>,
    /// The encoding every request asks unless it asks its own; when the
    /// answer does not say it, what its images are read as.
    #[serde(default)]
    pub output_format: Option<String>,
    /// How long a request may take to answer: two minutes unless given, a
    /// high-quality image taking more than the chat's default.
    #[serde(default = "two_minutes", deserialize_with = "seconds")]
    pub timeout: Duration,
    /// How long a connection may take to be made: five seconds unless
    /// given.
    #[serde(default = "five_seconds", deserialize_with = "seconds")]
    pub connect_timeout: Duration,
    /// The headers every request carries beside the provider's own.
    #[serde(default)]
    pub default_headers: Option<BTreeMap<String, String>>,
}

fn two_minutes() -> Duration {
    Duration::from_secs(120)
}

fn five_seconds() -> Duration {
    Duration::from_secs(5)
}

impl OpenAiImageConfig {
    /// The configuration of `model`, called with `api_key`, every other
    /// member its default.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: None,
            model: model.into(),
            quality: None,
            background: None,
            output_format: None,
            timeout: two_minutes(),
            connect_timeout: five_seconds(),
            default_headers: None,
        }
    }
}

impl fmt::Debug for OpenAiImageConfig {
    /// The configuration, its key and its headers' values never shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Option<Vec<&String>> = self
            .default_headers
            .as_ref()
            .map(|headers| headers.keys().collect());
        f.debug_struct("OpenAiImageConfig")
            .field("base_url", &self.base_url.as_deref().map(shown_url))
            .field("model", &self.model)
            .field("quality", &self.quality)
            .field("background", &self.background)
            .field("output_format", &self.output_format)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("default_headers", &headers)
            .finish_non_exhaustive()
    }
}
