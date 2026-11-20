//! How the request, a message and a part are read: from a JSON object only,
//! their fields as their Python models take them, a field left out taking
//! its default, and a part's `type` checked.

use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use super::AiRequest;
use super::message::{AiMessage, AiMessageContent, AiRole};
use super::parts::{AiImagePart, AiTextPart, AiThinkingPart, AiToolCallPart, AiToolResultPart};
use crate::Metadata;
use crate::schema::check_portable_object;
use crate::strict::read_as_object;
use crate::tools::{AiTool, ToolCallOutcome, ToolResult};
use crate::type_tag::type_tag;

read_as_object! {
    RequestForm => AiRequest,
    MessageForm => AiMessage,
    TextPartForm => AiTextPart,
    ImagePartForm => AiImagePart,
    ThinkingPartForm => AiThinkingPart,
    ToolCallPartForm => AiToolCallPart,
    ToolResultPartForm => AiToolResultPart,
}

/// [`AiRequest`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a request, as a JSON object")]
struct RequestForm {
    #[serde(default)]
    messages: Vec<AiMessage>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default = "default_temperature")]
    temperature: f64,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    thinking_budget: Option<i64>,
    #[serde(default)]
    enable_thinking: Option<bool>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    tools: Vec<AiTool>,
    #[serde(default, deserialize_with = "portable_schema")]
    response_schema: Option<Map<String, Value>>,
    #[serde(default)]
    metadata: Metadata,
}

impl From<RequestForm> for AiRequest {
    fn from(form: RequestForm) -> Self {
        Self {
            messages: form.messages,
            system_prompt: form.system_prompt,
            temperature: form.temperature,
            max_tokens: form.max_tokens,
            thinking_budget: form.thinking_budget,
            enable_thinking: form.enable_thinking,
            reasoning_effort: form.reasoning_effort,
            tools: form.tools,
            response_schema: form.response_schema,
            metadata: form.metadata,
        }
    }
}

/// The temperature of a request that states none.
fn default_temperature() -> f64 {
    AiRequest::default().temperature
}

/// Reads a request's response schema, refusing one outside the portable
/// subset in the words of
/// [`check_portable_schema`](crate::check_portable_schema), as Python
/// refuses it when a context is built.
fn portable_schema<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Map<String, Value>>, D::Error> {
    let schema = Option::<Map<String, Value>>::deserialize(deserializer)?;
    if let Some(schema) = &schema {
        check_portable_object(schema).map_err(serde::de::Error::custom)?;
    }
    Ok(schema)
}

/// [`AiMessage`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a message, as a JSON object")]
struct MessageForm {
    role: AiRole,
    content: AiMessageContent,
    #[serde(default)]
    metadata: Metadata,
}

impl From<MessageForm> for AiMessage {
    fn from(form: MessageForm) -> Self {
        Self {
            role: form.role,
            content: form.content,
            metadata: form.metadata,
        }
    }
}

type_tag!(TextType, "text");
type_tag!(ImageType, "image");
type_tag!(ThinkingType, "thinking");
type_tag!(ToolCallType, "tool_call");
type_tag!(ToolResultType, "tool_result");

/// [`AiTextPart`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a text part, as a JSON object")]
struct TextPartForm {
    #[serde(default, rename = "type")]
    _type: TextType,
    text: String,
}

impl From<TextPartForm> for AiTextPart {
    fn from(form: TextPartForm) -> Self {
        Self { text: form.text }
    }
}

/// [`AiImagePart`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "an image part, as a JSON object")]
struct ImagePartForm {
    #[serde(default, rename = "type")]
    _type: ImageType,
    url: String,
    #[serde(default)]
    mime_type: Option<String>,
}

impl From<ImagePartForm> for AiImagePart {
    fn from(form: ImagePartForm) -> Self {
        Self {
            url: form.url,
            mime_type: form.mime_type,
        }
    }
}

/// [`AiThinkingPart`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a thinking part, as a JSON object")]
struct ThinkingPartForm {
    #[serde(default, rename = "type")]
    _type: ThinkingType,
    thinking: String,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    redacted: Option<String>,
}

impl From<ThinkingPartForm> for AiThinkingPart {
    fn from(form: ThinkingPartForm) -> Self {
        Self {
            thinking: form.thinking,
            signature: form.signature,
            redacted: form.redacted,
        }
    }
}

/// [`AiToolCallPart`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a tool call part, as a JSON object")]
struct ToolCallPartForm {
    #[serde(default, rename = "type")]
    _type: ToolCallType,
    id: String,
    name: String,
    #[serde(default)]
    arguments: Metadata,
    #[serde(default)]
    metadata: Metadata,
}

impl From<ToolCallPartForm> for AiToolCallPart {
    fn from(form: ToolCallPartForm) -> Self {
        Self {
            id: form.id,
            name: form.name,
            arguments: form.arguments,
            metadata: form.metadata,
        }
    }
}

/// [`AiToolResultPart`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a tool result part, as a JSON object")]
struct ToolResultPartForm {
    #[serde(default, rename = "type")]
    _type: ToolResultType,
    tool_call_id: String,
    name: String,
    result: ToolResult,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    structured_content: Option<Metadata>,
    #[serde(default)]
    outcome: Option<ToolCallOutcome>,
    #[serde(default)]
    references: Vec<String>,
}

impl From<ToolResultPartForm> for AiToolResultPart {
    fn from(form: ToolResultPartForm) -> Self {
        Self {
            tool_call_id: form.tool_call_id,
            name: form.name,
            result: form.result,
            is_error: form.is_error,
            structured_content: form.structured_content,
            outcome: form.outcome,
            references: form.references,
        }
    }
}
