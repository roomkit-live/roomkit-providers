//! How a stream event is read: its fields as its Python model takes them, a
//! field left out taking its default, and its `type` checked.

use serde::Deserialize;

use super::calls::ServedCall;
use super::stream::{
    StreamDone, StreamTextDelta, StreamThinkingDelta, StreamToolCall, StreamToolCallDelta,
    StreamUsage,
};
use crate::Metadata;
use crate::type_tag::type_tag;

type_tag!(TextDeltaType, "text_delta");
type_tag!(ThinkingDeltaType, "thinking_delta");
type_tag!(ToolCallType, "tool_call");
type_tag!(ToolCallDeltaType, "tool_call_delta");
type_tag!(UsageType, "usage");
type_tag!(DoneType, "done");

/// [`StreamTextDelta`] as it is read.
#[derive(Deserialize)]
pub(super) struct TextDeltaForm {
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
pub(super) struct ThinkingDeltaForm {
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
pub(super) struct ToolCallForm {
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
pub(super) struct ToolCallDeltaForm {
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
pub(super) struct UsageForm {
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
pub(super) struct DoneForm {
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
