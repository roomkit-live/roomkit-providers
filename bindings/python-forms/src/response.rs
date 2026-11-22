//! An answer and the events of a stream: Python's `AIResponse` and its
//! `StreamEvent` union.

use roomkit::model::Metadata;
use roomkit_ai::{
    AiResponse, AiThinkingPart, AiToolCall, FinishReason, ServedCall, StreamDone, StreamEvent,
    StreamTextDelta, StreamThinkingDelta, StreamToolCall, StreamToolCallDelta, StreamUsage,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Form;
use crate::request::{ThinkingForm, thinking_part};

impl Form for AiResponse {
    /// What a host attaches to an answer afterwards, Python's `tasks` and
    /// `observations`, is not read: a provider's answer has none.
    fn read(form: Value) -> Result<Self, serde_json::Error> {
        ResponseForm::deserialize(form).map(Self::from)
    }

    fn written(&self) -> Value {
        let thinking_parts = self
            .thinking_parts
            .as_ref()
            .map(|parts| parts.iter().map(thinking_part).collect::<Vec<_>>());
        json!({
            "content": self.content,
            "thinking": self.thinking,
            "thinking_signature": self.thinking_signature,
            "thinking_parts": thinking_parts,
            "finish_reason": self.finish_reason,
            "usage": self.usage,
            "metadata": self.metadata,
            "tool_calls": self.tool_calls.iter().map(call).collect::<Vec<_>>(),
        })
    }
}

fn served(served: &ServedCall) -> Value {
    json!({ "result": served.result, "is_error": served.is_error })
}

fn call(call: &AiToolCall) -> Value {
    json!({
        "id": call.id,
        "name": call.name,
        "arguments": call.arguments,
        "metadata": call.metadata,
        "served": call.served.as_ref().map(served),
        "partial": call.partial,
        "garbled": call.garbled,
        "intent": call.intent,
    })
}

impl Form for StreamEvent {
    fn read(form: Value) -> Result<Self, serde_json::Error> {
        EventForm::deserialize(form).map(Self::from)
    }

    fn written(&self) -> Value {
        match self {
            Self::TextDelta(delta) => json!({ "type": "text_delta", "text": delta.text }),
            Self::ThinkingDelta(delta) => json!({
                "type": "thinking_delta",
                "thinking": delta.thinking,
                "signature": delta.signature,
                "block": delta.block,
                "redacted": delta.redacted,
            }),
            Self::ToolCall(call) => json!({
                "type": "tool_call",
                "id": call.id,
                "name": call.name,
                "arguments": call.arguments,
                "metadata": call.metadata,
                "served": call.served.as_ref().map(served),
                "partial": call.partial,
                "garbled": call.garbled,
                "intent": call.intent,
            }),
            Self::ToolCallDelta(delta) => json!({
                "type": "tool_call_delta",
                "id": delta.id,
                "name": delta.name,
                "index": delta.index,
                "arguments_delta": delta.arguments_delta,
            }),
            Self::Usage(usage) => json!({ "type": "usage", "usage": usage.usage }),
            Self::Done(done) => json!({
                "type": "done",
                "finish_reason": done.finish_reason,
                "usage": done.usage,
                "metadata": done.metadata,
            }),
        }
    }
}

/// [`AiResponse`] as it is read.
#[derive(Deserialize)]
struct ResponseForm {
    content: String,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    thinking_signature: Option<String>,
    #[serde(default)]
    thinking_parts: Option<Vec<ThinkingForm>>,
    /// A word of the contract, as Python's `AIResponse` takes no other.
    #[serde(default)]
    finish_reason: Option<FinishReason>,
    #[serde(default)]
    usage: Metadata,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    tool_calls: Vec<CallForm>,
}

impl From<ResponseForm> for AiResponse {
    fn from(form: ResponseForm) -> Self {
        let thinking_parts = form
            .thinking_parts
            .map(|parts| parts.into_iter().map(AiThinkingPart::from).collect());
        Self {
            content: form.content,
            finish_reason: form.finish_reason,
            usage: form.usage,
            metadata: form.metadata,
            tasks: Vec::new(),
            observations: Vec::new(),
            tool_calls: form.tool_calls.into_iter().map(AiToolCall::from).collect(),
            thinking: form.thinking,
            thinking_signature: form.thinking_signature,
            thinking_parts,
        }
    }
}

/// [`ServedCall`] as it is read.
#[derive(Deserialize)]
struct ServedForm {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
}

impl From<ServedForm> for ServedCall {
    fn from(form: ServedForm) -> Self {
        Self {
            result: form.result,
            is_error: form.is_error,
        }
    }
}

/// A call as it is read, in an answer ([`AiToolCall`]) and in a stream
/// ([`StreamToolCall`]).
#[derive(Deserialize)]
struct CallForm {
    id: String,
    name: String,
    #[serde(default)]
    arguments: Metadata,
    #[serde(default)]
    metadata: Metadata,
    #[serde(default)]
    served: Option<ServedForm>,
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
            partial: form.partial,
            garbled: form.garbled,
            intent: form.intent,
            served: form.served.map(ServedCall::from),
        }
    }
}

impl From<CallForm> for StreamToolCall {
    fn from(form: CallForm) -> Self {
        Self {
            id: form.id,
            name: form.name,
            arguments: form.arguments,
            metadata: form.metadata,
            partial: form.partial,
            garbled: form.garbled,
            intent: form.intent,
            served: form.served.map(ServedCall::from),
        }
    }
}

/// [`StreamEvent`] as it is read: the event its `type` names.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum EventForm {
    TextDelta {
        text: String,
    },
    ThinkingDelta {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
        #[serde(default)]
        block: Option<u32>,
        #[serde(default)]
        redacted: Option<String>,
    },
    ToolCall(CallForm),
    ToolCallDelta {
        id: String,
        name: String,
        #[serde(default)]
        index: u32,
        #[serde(default)]
        arguments_delta: String,
    },
    Usage {
        #[serde(default)]
        usage: Metadata,
    },
    Done {
        #[serde(default)]
        finish_reason: Option<FinishReason>,
        #[serde(default)]
        usage: Metadata,
        #[serde(default)]
        metadata: Metadata,
    },
}

impl From<EventForm> for StreamEvent {
    fn from(form: EventForm) -> Self {
        match form {
            EventForm::TextDelta { text } => Self::TextDelta(StreamTextDelta { text }),
            EventForm::ThinkingDelta {
                thinking,
                signature,
                block,
                redacted,
            } => Self::ThinkingDelta(StreamThinkingDelta {
                thinking,
                signature,
                block,
                redacted,
            }),
            EventForm::ToolCall(call) => Self::ToolCall(call.into()),
            EventForm::ToolCallDelta {
                id,
                name,
                index,
                arguments_delta,
            } => Self::ToolCallDelta(StreamToolCallDelta {
                id,
                name,
                index,
                arguments_delta,
            }),
            EventForm::Usage { usage } => Self::Usage(StreamUsage { usage }),
            EventForm::Done {
                finish_reason,
                usage,
                metadata,
            } => Self::Done(StreamDone {
                finish_reason,
                usage,
                metadata,
            }),
        }
    }
}
