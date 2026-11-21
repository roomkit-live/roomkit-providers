//! How a response, a call and a stream event are read: from a JSON object
//! only, their fields as their Python models take them, a field left out
//! taking its default, and an event's `type` checked.

use serde::Deserialize;

use super::AiResponse;
use super::calls::{AiToolCall, ServedCall};
use super::stream::{
    StreamDone, StreamTextDelta, StreamThinkingDelta, StreamToolCall, StreamToolCallDelta,
    StreamUsage,
};
use crate::Metadata;
use crate::request::AiThinkingPart;
use crate::strict::read_as_object;
use crate::type_tag::type_tag;

read_as_object! {
    ResponseForm => AiResponse,
    CallForm => AiToolCall,
    ServedCallForm => ServedCall,
    TextDeltaForm => StreamTextDelta,
    ThinkingDeltaForm => StreamThinkingDelta,
    ToolCallForm => StreamToolCall,
    ToolCallDeltaForm => StreamToolCallDelta,
    UsageForm => StreamUsage,
    DoneForm => StreamDone,
}

/// [`AiResponse`] as it is read. Python's `tasks` and `observations`, which
/// a host attaches, are left out.
#[derive(Deserialize)]
#[serde(expecting = "a response, as a JSON object")]
struct ResponseForm {
    content: String,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    thinking_signature: Option<String>,
    #[serde(default)]
    thinking_parts: Option<Vec<AiThinkingPart>>,
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default, deserialize_with = "crate::usage::read")]
    usage: Metadata,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    tool_calls: Vec<AiToolCall>,
}

impl From<ResponseForm> for AiResponse {
    fn from(form: ResponseForm) -> Self {
        Self {
            content: form.content,
            thinking: form.thinking,
            thinking_signature: form.thinking_signature,
            thinking_parts: form.thinking_parts,
            finish_reason: form.finish_reason,
            usage: form.usage,
            metadata: form.metadata,
            tool_calls: form.tool_calls,
        }
    }
}

/// [`AiToolCall`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a tool call, as a JSON object")]
struct CallForm {
    id: String,
    name: String,
    #[serde(default)]
    arguments: Metadata,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    served: Option<ServedCall>,
    #[serde(default)]
    partial: bool,
    #[serde(default)]
    garbled: bool,
    #[serde(default)]
    intent: Option<String>,
}

impl From<CallForm> for AiToolCall {
    fn from(form: CallForm) -> Self {
        Self {
            id: form.id,
            name: form.name,
            arguments: form.arguments,
            metadata: form.metadata,
            served: form.served,
            partial: form.partial,
            garbled: form.garbled,
            intent: form.intent,
        }
    }
}

/// [`ServedCall`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a served call, as a JSON object")]
struct ServedCallForm {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
}

impl From<ServedCallForm> for ServedCall {
    fn from(form: ServedCallForm) -> Self {
        Self {
            result: form.result,
            is_error: form.is_error,
        }
    }
}

type_tag!(TextDeltaType, "text_delta");
type_tag!(ThinkingDeltaType, "thinking_delta");
type_tag!(ToolCallType, "tool_call");
type_tag!(ToolCallDeltaType, "tool_call_delta");
type_tag!(UsageType, "usage");
type_tag!(DoneType, "done");

/// [`StreamTextDelta`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a text delta, as a JSON object")]
struct TextDeltaForm {
    #[serde(default, rename = "type")]
    _type: TextDeltaType,
    text: String,
}

impl From<TextDeltaForm> for StreamTextDelta {
    fn from(form: TextDeltaForm) -> Self {
        Self { text: form.text }
    }
}

/// [`StreamThinkingDelta`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a thinking delta, as a JSON object")]
struct ThinkingDeltaForm {
    #[serde(default, rename = "type")]
    _type: ThinkingDeltaType,
    thinking: String,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    block: Option<u32>,
    #[serde(default)]
    redacted: Option<String>,
}

impl From<ThinkingDeltaForm> for StreamThinkingDelta {
    fn from(form: ThinkingDeltaForm) -> Self {
        Self {
            thinking: form.thinking,
            signature: form.signature,
            block: form.block,
            redacted: form.redacted,
        }
    }
}

/// [`StreamToolCall`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a streamed tool call, as a JSON object")]
struct ToolCallForm {
    #[serde(default, rename = "type")]
    _type: ToolCallType,
    id: String,
    name: String,
    #[serde(default)]
    arguments: Metadata,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    served: Option<ServedCall>,
    #[serde(default)]
    partial: bool,
    #[serde(default)]
    garbled: bool,
    #[serde(default)]
    intent: Option<String>,
}

impl From<ToolCallForm> for StreamToolCall {
    fn from(form: ToolCallForm) -> Self {
        Self {
            id: form.id,
            name: form.name,
            arguments: form.arguments,
            metadata: form.metadata,
            served: form.served,
            partial: form.partial,
            garbled: form.garbled,
            intent: form.intent,
        }
    }
}

/// [`StreamToolCallDelta`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a tool call delta, as a JSON object")]
struct ToolCallDeltaForm {
    #[serde(default, rename = "type")]
    _type: ToolCallDeltaType,
    id: String,
    name: String,
    #[serde(default)]
    index: u32,
    #[serde(default)]
    arguments_delta: String,
}

impl From<ToolCallDeltaForm> for StreamToolCallDelta {
    fn from(form: ToolCallDeltaForm) -> Self {
        Self {
            id: form.id,
            name: form.name,
            index: form.index,
            arguments_delta: form.arguments_delta,
        }
    }
}

/// [`StreamUsage`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a usage event, as a JSON object")]
struct UsageForm {
    #[serde(default, rename = "type")]
    _type: UsageType,
    #[serde(default, deserialize_with = "crate::usage::read")]
    usage: Metadata,
}

impl From<UsageForm> for StreamUsage {
    fn from(form: UsageForm) -> Self {
        Self { usage: form.usage }
    }
}

/// [`StreamDone`] as it is read.
#[derive(Deserialize)]
#[serde(expecting = "a done event, as a JSON object")]
struct DoneForm {
    #[serde(default, rename = "type")]
    _type: DoneType,
    #[serde(default)]
    finish_reason: Option<String>,
    #[serde(default, deserialize_with = "crate::usage::read")]
    usage: Metadata,
    #[serde(default)]
    metadata: Metadata,
}

impl From<DoneForm> for StreamDone {
    fn from(form: DoneForm) -> Self {
        Self {
            finish_reason: form.finish_reason,
            usage: form.usage,
            metadata: form.metadata,
        }
    }
}
