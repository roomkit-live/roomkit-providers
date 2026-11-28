//! What an OpenRouter image provider is configured with: Python's
//! `OpenRouterImageConfig`, and its JSON form.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::config::form::seconds;
use crate::endpoint::shown_url;

/// The configuration of an [`OpenRouterImage`](super::OpenRouterImage):
/// Python's `OpenRouterImageConfig`, `OpenAIImageConfig`'s members with
/// OpenRouter's endpoint and its attribution headers.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(
    deny_unknown_fields,
    expecting = "an OpenRouter image configuration, as a JSON object"
)]
pub struct OpenRouterImageConfig {
    /// The key every request carries as a bearer.
    pub api_key: String,
    /// OpenRouter's endpoint: `https://openrouter.ai/api/v1` unless given.
    #[serde(default = "openrouter")]
    pub base_url: String,
    /// The routed model (`openai/gpt-image-2`).
    pub model: String,
    /// The rendering effort every request asks.
    #[serde(default)]
    pub quality: Option<String>,
    /// The background every request asks.
    #[serde(default)]
    pub background: Option<String>,
    /// The encoding every request asks.
    #[serde(default)]
    pub output_format: Option<String>,
    /// How long an answer may take: two minutes unless given.
    #[serde(default = "two_minutes", deserialize_with = "seconds")]
    pub timeout: Duration,
    /// How long a connection may take: five seconds unless given.
    #[serde(default = "five_seconds", deserialize_with = "seconds")]
    pub connect_timeout: Duration,
    /// Headers every request carries, which replace the provider's of the
    /// same name.
    #[serde(default)]
    pub default_headers: Option<BTreeMap<String, String>>,
    /// The site sent as `HTTP-Referer`, OpenRouter's attribution.
    #[serde(default)]
    pub site_url: Option<String>,
    /// The application named in `X-Title`, OpenRouter's attribution.
    #[serde(default)]
    pub app_name: Option<String>,
}

fn openrouter() -> String {
    "https://openrouter.ai/api/v1".to_owned()
}

fn two_minutes() -> Duration {
    Duration::from_secs(120)
}

fn five_seconds() -> Duration {
    Duration::from_secs(5)
}

impl OpenRouterImageConfig {
    /// The configuration called with `api_key` and `model`, every other
    /// member its default.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: openrouter(),
            model: model.into(),
            quality: None,
            background: None,
            output_format: None,
            timeout: two_minutes(),
            connect_timeout: five_seconds(),
            default_headers: None,
            site_url: None,
            app_name: None,
        }
    }
}

impl fmt::Debug for OpenRouterImageConfig {
    /// The configuration, its key and its headers' values never shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Option<Vec<&String>> = self
            .default_headers
            .as_ref()
            .map(|headers| headers.keys().collect());
        f.debug_struct("OpenRouterImageConfig")
            .field("base_url", &shown_url(&self.base_url))
            .field("model", &self.model)
            .field("quality", &self.quality)
            .field("background", &self.background)
            .field("output_format", &self.output_format)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .field("default_headers", &headers)
            .field("site_url", &self.site_url.as_deref().map(shown_url))
            .field("app_name", &self.app_name)
            .finish_non_exhaustive()
    }
}
