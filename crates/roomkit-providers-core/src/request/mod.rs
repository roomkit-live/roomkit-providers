//! What one generation is asked (RFC §6.7): the request and its messages.
//!
//! - [`AiRequest`]: the conversation, the sampling and reasoning settings,
//!   the tools and the data of one generation.
//! - [`AiMessage`], [`AiRole`] and [`AiMessageContent`]: one message of the
//!   conversation.
//! - [`AiContentPart`] and its five parts: [`AiTextPart`], [`AiImagePart`],
//!   [`AiThinkingPart`], [`AiToolCallPart`] and [`AiToolResultPart`].

mod message;
mod part_forms;
mod parts;

use std::fmt;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

pub use self::message::{AiMessage, AiMessageContent, AiRole};
pub use self::parts::{
    AiContentPart, AiImagePart, AiTextPart, AiThinkingPart, AiToolCallPart, AiToolResultPart,
};
use crate::Metadata;
use crate::tools::AiTool;

/// The key of [`AiRequest::metadata`] that holds the credential of one
/// request.
const API_KEY: &str = "api_key";

/// What stands for the credential in a request's JSON form: what Python
/// writes for a secret string.
const MASK: &str = "**********";

/// What one generation is asked (RFC §6.7): the fields a provider reads of
/// Python's `AIContext(*, messages=[], system_prompt=None, temperature=0.7,
/// max_tokens=None, thinking_budget=None, enable_thinking=None,
/// reasoning_effort=None, tools=[], response_schema=None, metadata={})`.
///
/// A request is built by struct update over its defaults:
///
/// ```
/// use roomkit_providers_core::{AiMessage, AiRequest, AiRole};
///
/// let request = AiRequest {
///     messages: vec![AiMessage::new(AiRole::User, "Where is my order?")],
///     system_prompt: Some("You answer in one sentence.".into()),
///     ..AiRequest::default()
/// };
/// assert_eq!(request.temperature, 0.7);
/// ```
///
/// # The credential of one request
///
/// A host may leave the credential to use for one request under `api_key`
/// in [`metadata`](Self::metadata). The JSON form of a request never
/// carries it: a non-empty string there is written as `"**********"`, as
/// Python writes it, and the `Debug` form masks it too. A host that passes
/// a request across a boundary as JSON passes the key apart.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiRequest {
    /// The conversation, oldest first, the turn's rounds so far included.
    pub messages: Vec<AiMessage>,
    /// The system prompt, when there is one.
    pub system_prompt: Option<String>,
    /// How freely the model samples: Python's `float`, 0.7 by default.
    pub temperature: f64,
    /// The most tokens the generation writes; `None` leaves it to the
    /// provider's configuration.
    pub max_tokens: Option<u32>,
    /// The tokens the model may reason with this turn (RFC §6.7): 0 states
    /// off, above 0 on; `None` leaves it to `enable_thinking`.
    pub thinking_budget: Option<i64>,
    /// Whether the model reasons this turn (RFC §6.7), for a provider that
    /// exposes the switch; `None` defers to its configuration, then to the
    /// model's default.
    pub enable_thinking: Option<bool>,
    /// How hard a reasoning model thinks this turn (RFC §6.7), which a
    /// provider prefers over its configured effort; `None` leaves it to the
    /// provider's configuration.
    pub reasoning_effort: Option<String>,
    /// The tools the model may call, in the toolset's order.
    pub tools: Vec<AiTool>,
    /// The JSON Schema the answer satisfies (RFC §6.7), within the subset of
    /// JSON Schema the RFC names portable. A provider handed one answers
    /// with one JSON document or fails; it never ignores it.
    pub response_schema: Option<Map<String, Value>>,
    /// The turn's data for the provider.
    #[serde(serialize_with = "with_credential_masked")]
    pub metadata: Metadata,
}

/// Writes a request's metadata with its credential masked, as Python does:
/// a non-empty string under `api_key` is written as the mask. An empty
/// string or a value that is no string is no credential and is written as
/// it is, and so is an `api_key` anywhere deeper.
fn with_credential_masked<S: Serializer>(
    metadata: &Metadata,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut written = serializer.serialize_map(Some(metadata.len()))?;
    for (key, value) in metadata {
        if key == API_KEY && is_credential(value) {
            written.serialize_entry(key, MASK)?;
        } else {
            written.serialize_entry(key, value)?;
        }
    }
    written.end()
}

/// Whether `value`, found under `api_key`, is a credential: a non-empty
/// string.
fn is_credential(value: &Value) -> bool {
    value.as_str().is_some_and(|key| !key.is_empty())
}

impl fmt::Debug for AiRequest {
    /// The credential of the request is left out: an `api_key` entry of the
    /// metadata is shown masked.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut metadata = self.metadata.clone();
        if metadata.contains_key(API_KEY) {
            metadata.insert(API_KEY.into(), "[REDACTED]".into());
        }
        formatter
            .debug_struct("AiRequest")
            .field("messages", &self.messages)
            .field("system_prompt", &self.system_prompt)
            .field("temperature", &self.temperature)
            .field("max_tokens", &self.max_tokens)
            .field("thinking_budget", &self.thinking_budget)
            .field("enable_thinking", &self.enable_thinking)
            .field("reasoning_effort", &self.reasoning_effort)
            .field("tools", &self.tools)
            .field("response_schema", &self.response_schema)
            .field("metadata", &metadata)
            .finish()
    }
}

impl Default for AiRequest {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
            system_prompt: None,
            temperature: 0.7,
            max_tokens: None,
            thinking_budget: None,
            enable_thinking: None,
            reasoning_effort: None,
            tools: Vec::new(),
            response_schema: None,
            metadata: Metadata::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PLACEHOLDER: &str = "placeholder-not-a-key";

    #[test]
    fn a_request_s_debug_form_masks_its_credential() {
        let mut request = AiRequest::default();
        request.metadata.insert("tenant".into(), json!("acme"));
        request.metadata.insert(API_KEY.into(), json!(PLACEHOLDER));
        let shown = format!("{request:?}");
        assert!(!shown.contains(PLACEHOLDER), "{shown}");
        assert!(
            shown.contains("[REDACTED]") && shown.contains("acme"),
            "{shown}"
        );
        // Masking changes what is shown, never the request.
        assert_eq!(request.metadata.get(API_KEY), Some(&json!(PLACEHOLDER)));
    }

    // Python: AIContext(metadata={"api_key": ...}) keeps the key for its
    // provider and dumps it masked.
    #[test]
    fn a_request_reads_its_credential_and_never_writes_it() {
        let request: AiRequest = serde_json::from_value(json!({
            "metadata": { "tenant": "acme", "api_key": PLACEHOLDER },
        }))
        .unwrap();
        assert_eq!(request.metadata.get(API_KEY), Some(&json!(PLACEHOLDER)));
        let written = serde_json::to_string(&request).unwrap();
        assert!(!written.contains(PLACEHOLDER), "{written}");
        assert!(
            written.ends_with(r#""metadata":{"tenant":"acme","api_key":"**********"}}"#),
            "{written}"
        );
    }
}
