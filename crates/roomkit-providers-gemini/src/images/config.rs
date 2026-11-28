//! What a Gemini image provider is configured with: Python's
//! `GeminiImageConfig`, and its JSON form.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

use crate::config::seconds;

/// The configuration of a [`GeminiImage`](super::GeminiImage): Python's
/// `GeminiImageConfig`. Apart from the conversational configuration, as
/// Python keeps it: the Interactions API, of image models disjoint from the
/// conversational ones (RFC §25.6), on Gemini's Developer API.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(
    deny_unknown_fields,
    expecting = "a Gemini image configuration, as a JSON object"
)]
pub struct GeminiImageConfig {
    /// The key every request carries in `x-goog-api-key`.
    pub api_key: String,
    /// The image model: `gemini-nano-banana-2.1` unless given.
    #[serde(default = "nano_banana")]
    pub model: String,
    /// The resolution tier every request asks unless it asks its own
    /// (`512`, `1K`, `2K`, `4K`).
    #[serde(default)]
    pub image_size: Option<String>,
    /// The media type every request asks unless it asks its own
    /// (`image/jpeg`).
    #[serde(default)]
    pub output_mime_type: Option<String>,
    /// How long an interaction may take to answer: two minutes unless
    /// given.
    #[serde(default = "two_minutes", deserialize_with = "seconds")]
    pub timeout: Duration,
    /// How long a connection may take to be made: five seconds unless
    /// given.
    #[serde(default = "five_seconds", deserialize_with = "seconds")]
    pub connect_timeout: Duration,
}

fn nano_banana() -> String {
    "gemini-nano-banana-2.1".to_owned()
}

fn two_minutes() -> Duration {
    Duration::from_secs(120)
}

fn five_seconds() -> Duration {
    Duration::from_secs(5)
}

impl GeminiImageConfig {
    /// The configuration called with `api_key`, every other member its
    /// default.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: nano_banana(),
            image_size: None,
            output_mime_type: None,
            timeout: two_minutes(),
            connect_timeout: five_seconds(),
        }
    }
}

impl fmt::Debug for GeminiImageConfig {
    /// The configuration, its key never shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeminiImageConfig")
            .field("model", &self.model)
            .field("image_size", &self.image_size)
            .field("output_mime_type", &self.output_mime_type)
            .field("timeout", &self.timeout)
            .field("connect_timeout", &self.connect_timeout)
            .finish_non_exhaustive()
    }
}
