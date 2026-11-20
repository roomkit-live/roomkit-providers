//! The events of a structured stream (RFC §6.7, Appendix A.9).
//!
//! Each event writes its `type` first, as its Python model does. Read alone,
//! an event takes a missing `type` as its own and refuses any other; read as
//! a [`StreamEvent`], its `type` says which event it is.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::calls::{AiToolCall, ServedCall};
use crate::Metadata;
use crate::strict::{Tagged, read_tagged};

/// One event of a structured stream (RFC §6.7, Appendix A.9): Python's
/// `StreamEvent`, its classes told apart by their `type`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// Text the model wrote, as it came.
    TextDelta(StreamTextDelta),
    /// A call the model made.
    ToolCall(StreamToolCall),
    /// The generation ended.
    Done(StreamDone),
    /// A piece of the model's reasoning, before its text (RFC §6.7).
    ThinkingDelta(StreamThinkingDelta),
    /// The opening or an argument fragment of a call, before its whole value.
    ToolCallDelta(StreamToolCallDelta),
    /// What the generation used so far, as its provider told it before its
    /// end (RFC §6.4).
    Usage(StreamUsage),
}

impl Serialize for StreamEvent {
    /// Writes the event as its own model does, `type` included.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::TextDelta(event) => event.serialize(serializer),
            Self::ToolCall(event) => event.serialize(serializer),
            Self::Done(event) => event.serialize(serializer),
            Self::ThinkingDelta(event) => event.serialize(serializer),
            Self::ToolCallDelta(event) => event.serialize(serializer),
            Self::Usage(event) => event.serialize(serializer),
        }
    }
}

impl Tagged for StreamEvent {
    const EXPECTING: &'static str = "a stream event, as a JSON object with its `type`";
    const TYPES: &'static [&'static str] = &[
        "text_delta",
        "tool_call",
        "done",
        "thinking_delta",
        "tool_call_delta",
        "usage",
    ];

    fn read<'de, D: Deserializer<'de>>(tag: &str, rest: D) -> Option<Result<Self, D::Error>> {
        Some(match tag {
            "text_delta" => StreamTextDelta::deserialize(rest).map(Self::TextDelta),
            "tool_call" => StreamToolCall::deserialize(rest).map(Self::ToolCall),
            "done" => StreamDone::deserialize(rest).map(Self::Done),
            "thinking_delta" => StreamThinkingDelta::deserialize(rest).map(Self::ThinkingDelta),
            "tool_call_delta" => StreamToolCallDelta::deserialize(rest).map(Self::ToolCallDelta),
            "usage" => StreamUsage::deserialize(rest).map(Self::Usage),
            _ => return None,
        })
    }
}

impl<'de> Deserialize<'de> for StreamEvent {
    /// Reads an event from an object, its `type` a string saying which.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        read_tagged(deserializer)
    }
}

/// A streaming call fragment: Python's `StreamToolCallDelta(*, id, name,
/// index=0, arguments_delta="")`. The whole [`StreamToolCall`] still
/// follows and remains what runs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "tool_call_delta")]
pub struct StreamToolCallDelta {
    /// The stable identity of the call throughout this response.
    pub id: String,
    /// The tool being called.
    pub name: String,
    /// What tells the response's calls apart: the call's position among
    /// them, or the provider's content block index.
    pub index: u32,
    /// An incremental fragment of the JSON arguments, not valid JSON on its
    /// own; empty at opening.
    pub arguments_delta: String,
}

impl StreamToolCallDelta {
    /// Open a call with no arguments received yet.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            ..Self::default()
        }
    }
}

/// A piece of a model's reasoning: Python's `StreamThinkingDelta(*, thinking,
/// signature=None, block=None, redacted=None)`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "thinking_delta")]
pub struct StreamThinkingDelta {
    /// The reasoning text; empty for a delta that only carries a signature.
    pub thinking: String,
    /// The vendor's opaque token for the block, which it wants back with
    /// the block when a round is replayed.
    pub signature: Option<String>,
    /// The reasoning block the delta belongs to, for a vendor whose
    /// reasoning comes in blocks each with its own signature; `None` for a
    /// single block per round.
    pub block: Option<u32>,
    /// A block the vendor redacted: its opaque data, replayed as received.
    pub redacted: Option<String>,
}

impl StreamThinkingDelta {
    /// The delta `thinking`.
    pub fn new(thinking: impl Into<String>) -> Self {
        Self {
            thinking: thinking.into(),
            ..Self::default()
        }
    }
}

/// Text the model wrote: Python's `StreamTextDelta(*, text)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "text_delta")]
pub struct StreamTextDelta {
    /// The text.
    pub text: String,
}

impl StreamTextDelta {
    /// The delta `text`.
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

/// A call the model made: Python's `StreamToolCall`, the fields of
/// [`AiToolCall`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "tool_call")]
pub struct StreamToolCall {
    /// The call's id.
    pub id: String,
    /// The tool called.
    pub name: String,
    /// Its arguments, a mapping.
    pub arguments: Metadata,
    /// Provider-owned data of the call.
    pub metadata: Metadata,
    /// An outcome the provider already produced; `None` leaves the call pending.
    pub served: Option<ServedCall>,
    /// The call never runs.
    pub partial: bool,
    /// The model wrote the arguments unreadable.
    pub garbled: bool,
    /// What the call does, for the person.
    pub intent: Option<String>,
}

impl StreamToolCall {
    /// The call `id` of the tool `name`, with no arguments.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        AiToolCall::new(id, name).into()
    }
}

/// Copy a tool call into an independent streamed event, preserving every field.
pub fn stream_call_of(call: &AiToolCall) -> StreamToolCall {
    call.clone().into()
}

/// Copy a streamed event into an independent tool call, preserving every field.
pub fn tool_call_of(event: &StreamToolCall) -> AiToolCall {
    event.clone().into()
}

impl From<AiToolCall> for StreamToolCall {
    fn from(call: AiToolCall) -> Self {
        let AiToolCall {
            id,
            name,
            arguments,
            metadata,
            served,
            partial,
            garbled,
            intent,
        } = call;
        Self {
            id,
            name,
            arguments,
            metadata,
            served,
            partial,
            garbled,
            intent,
        }
    }
}

impl From<StreamToolCall> for AiToolCall {
    fn from(call: StreamToolCall) -> Self {
        let StreamToolCall {
            id,
            name,
            arguments,
            metadata,
            served,
            partial,
            garbled,
            intent,
        } = call;
        Self {
            id,
            name,
            arguments,
            metadata,
            served,
            partial,
            garbled,
            intent,
        }
    }
}

/// What a generation used so far, as its provider told it before its end:
/// Python's `StreamUsage(*, usage={})`. A provider that tells its input
/// counters as its stream starts yields them here; a generation cut before
/// its end counts them, and the usage of its [`StreamDone`], when it comes,
/// replaces them (RFC §6.4).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "usage")]
pub struct StreamUsage {
    /// What it used so far: integer counters by name.
    pub usage: Metadata,
}

/// How a generation ended: Python's `StreamDone(*, finish_reason=None,
/// usage={}, metadata={})`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename = "done")]
pub struct StreamDone {
    /// Why it stopped.
    pub finish_reason: Option<String>,
    /// What it used: integer counters by name.
    pub usage: Metadata,
    /// As [`AiResponse::metadata`](super::AiResponse::metadata): what the
    /// response says of itself (`model`, `refusal`) and provider-owned data
    /// for replaying a tool round.
    pub metadata: Metadata,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_call_and_its_streamed_event_copy_every_field_both_ways() {
        let mut call = AiToolCall::new("c1", "lookup");
        call.arguments.insert("order".into(), json!("A-1001"));
        call.metadata.insert("index".into(), json!(0));
        call.served = Some(ServedCall {
            result: "shipped".into(),
            is_error: false,
        });
        call.partial = true;
        call.garbled = true;
        call.intent = Some("Look the order up".into());
        let event = stream_call_of(&call);
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({
                "type": "tool_call",
                "id": "c1",
                "name": "lookup",
                "arguments": { "order": "A-1001" },
                "metadata": { "index": 0 },
                "served": { "result": "shipped", "is_error": false },
                "partial": true,
                "garbled": true,
                "intent": "Look the order up",
            })
        );
        assert_eq!(tool_call_of(&event), call);
    }

    // Python: StreamTextDelta.model_validate({"text": "hi"}) takes its own
    // type; {"type": "done", "text": "hi"} is refused.
    #[test]
    fn an_event_read_alone_takes_its_own_type_and_refuses_another() {
        assert_eq!(
            serde_json::from_value::<StreamTextDelta>(json!({ "text": "hi" })).unwrap(),
            StreamTextDelta::new("hi")
        );
        assert!(
            serde_json::from_value::<StreamTextDelta>(json!({ "type": "done", "text": "hi" }))
                .is_err()
        );
        assert!(serde_json::from_value::<StreamDone>(json!({ "type": "usage" })).is_err());
    }

    #[test]
    fn an_event_of_a_stream_is_told_apart_by_its_type() {
        let read = |value| serde_json::from_value::<StreamEvent>(value);
        assert_eq!(
            read(json!({ "type": "done" })).unwrap(),
            StreamEvent::Done(StreamDone::default())
        );
        assert_eq!(
            read(json!({ "type": "tool_call_delta", "id": "c1", "name": "lookup" })).unwrap(),
            StreamEvent::ToolCallDelta(StreamToolCallDelta::new("c1", "lookup"))
        );
        for refused in [
            json!({ "type": "audio_delta", "data": "" }),
            json!({ "text": "no type" }),
            json!({ "type": "text_delta" }),
        ] {
            assert!(read(refused.clone()).is_err(), "{refused}");
        }
    }

    // Python: StreamUsage(usage={"input_tokens": 1.5}) is refused.
    #[test]
    fn a_usage_counter_that_is_no_integer_is_refused_on_input() {
        let read = |usage| serde_json::from_value::<StreamUsage>(json!({ "usage": usage }));
        assert!(read(json!({ "input_tokens": 12, "delta": -3 })).is_ok());
        for refused in [
            json!({ "input_tokens": 1.5 }),
            json!({ "input_tokens": "12" }),
            json!({ "input_tokens": null }),
            json!({ "details": { "cached": 1 } }),
        ] {
            let error = read(refused.clone()).unwrap_err().to_string();
            assert!(error.contains("must be an integer"), "{refused}: {error}");
        }
        assert!(
            serde_json::from_value::<StreamDone>(json!({ "usage": { "output_tokens": true } }))
                .is_err()
        );
    }
}
