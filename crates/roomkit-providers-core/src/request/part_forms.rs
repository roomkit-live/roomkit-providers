//! How a part is read: its fields as its Python model takes them, a field
//! left out taking its default, and its `type` checked.

use serde::Deserialize;

use super::parts::{AiImagePart, AiTextPart, AiThinkingPart, AiToolCallPart, AiToolResultPart};
use crate::Metadata;
use crate::tools::{ToolCallOutcome, ToolResult};
use crate::type_tag::type_tag;

type_tag!(TextType, "text");
type_tag!(ImageType, "image");
type_tag!(ThinkingType, "thinking");
type_tag!(ToolCallType, "tool_call");
type_tag!(ToolResultType, "tool_result");

/// [`AiTextPart`] as it is read.
#[derive(Deserialize)]
pub(super) struct TextPartForm {
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
pub(super) struct ImagePartForm {
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
pub(super) struct ThinkingPartForm {
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
pub(super) struct ToolCallPartForm {
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
pub(super) struct ToolResultPartForm {
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
