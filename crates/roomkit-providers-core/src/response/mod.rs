//! What a generation answers (RFC §6.7): the whole response, the calls it
//! makes, and the events of a structured stream.
//!
//! - [`AiResponse`]: a whole generation, with [`thinking_parts_of`] to read
//!   its reasoning blocks.
//! - [`AiToolCall`] and [`ServedCall`]: a call the model made, and the
//!   outcome of one its provider already ran.
//! - [`CallIds`], [`tool_arguments`] and [`is_truncation`]: the one reading
//!   of a call's id, of its arguments and of a response's ending every
//!   provider applies (RFC §6.4).
//! - [`StreamEvent`] and its six events, with [`stream_call_of`] and
//!   [`tool_call_of`] between a call and its streamed event.

mod calls;
mod event_forms;
mod stream;

use serde::{Deserialize, Serialize};

pub use self::calls::{AiToolCall, CallIds, ServedCall, is_truncation, tool_arguments};
pub use self::stream::{
    StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta, StreamToolCall,
    StreamToolCallDelta, StreamUsage, stream_call_of, tool_call_of,
};
use crate::Metadata;
use crate::request::AiThinkingPart;

/// A whole generation (RFC §6.7): the fields a provider fills of Python's
/// `AIResponse(*, content, thinking=None, thinking_signature=None,
/// thinking_parts=None, finish_reason=None, usage={}, metadata={},
/// tool_calls=[])`. Python's `tasks` and `observations`, which a host
/// attaches, are not part of it: read from JSON, they are left out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiResponse {
    /// Its text.
    pub content: String,
    /// All the model's reasoning text (RFC §6.7), none when it showed none.
    #[serde(default)]
    pub thinking: Option<String>,
    /// The last signature of its reasoning (RFC §6.4).
    #[serde(default)]
    pub thinking_signature: Option<String>,
    /// The reasoning blocks to replay, in the order the vendor sent them,
    /// each with its signature or redacted data (RFC §6.4); `None` for a
    /// provider whose reasoning has no blocks. A block the response cut before
    /// its signature is not among them.
    #[serde(default)]
    pub thinking_parts: Option<Vec<AiThinkingPart>>,
    /// Why it stopped, in the provider's own word (`stop`, `tool_calls`,
    /// `length`...); `None` when it gave none.
    #[serde(default)]
    pub finish_reason: Option<String>,
    /// What it used: integer counters by name (`input_tokens`,
    /// `output_tokens`...).
    #[serde(default, deserialize_with = "crate::usage::read")]
    pub usage: Metadata,
    /// What the response says of itself, `model` the model that answered
    /// and `refusal` the refusal it carried, and provider-owned data for
    /// replaying a tool round.
    #[serde(default)]
    pub metadata: Metadata,
    /// The calls it made, in order.
    #[serde(default)]
    pub tool_calls: Vec<AiToolCall>,
}

impl AiResponse {
    /// A response saying `content`, with no call, ending for no reason.
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            thinking: None,
            thinking_signature: None,
            thinking_parts: None,
            finish_reason: None,
            usage: Metadata::new(),
            metadata: Metadata::new(),
            tool_calls: Vec::new(),
        }
    }
}

/// A response's reasoning blocks (RFC §6.4): its `thinking_parts`, or for a
/// provider that reports one block its `thinking` with its signature. Python's
/// `thinking_parts_of(response)`.
#[must_use]
pub fn thinking_parts_of(response: &AiResponse) -> Vec<AiThinkingPart> {
    if let Some(parts) = &response.thinking_parts {
        return parts.clone();
    }
    let thinking = response.thinking.as_deref().unwrap_or_default();
    let signature = response.thinking_signature.as_deref().unwrap_or_default();
    if thinking.is_empty() && signature.is_empty() {
        return Vec::new();
    }
    vec![AiThinkingPart {
        signature: response.thinking_signature.clone(),
        ..AiThinkingPart::new(thinking)
    }]
}

/// The events `response` stands for: each reasoning block with its signature
/// or redacted data (a signature alone too), its text, its calls, then its
/// end, as Python's `response_stream_events` wraps a whole response.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "what calls it, a provider's default structured stream, is not defined yet"
    )
)]
pub(crate) fn response_events(response: AiResponse) -> Vec<StreamEvent> {
    let blocks = thinking_parts_of(&response);
    let named = response.thinking_parts.is_some();
    let AiResponse {
        content,
        finish_reason,
        usage,
        metadata,
        tool_calls,
        ..
    } = response;
    let mut events: Vec<StreamEvent> = blocks
        .into_iter()
        .enumerate()
        .map(|(block, part)| {
            StreamEvent::ThinkingDelta(StreamThinkingDelta {
                thinking: part.thinking,
                signature: part.signature,
                redacted: part.redacted,
                block: if named {
                    u32::try_from(block).ok()
                } else {
                    None
                },
            })
        })
        .collect();
    if !content.is_empty() {
        events.push(StreamEvent::TextDelta(StreamTextDelta { text: content }));
    }
    events.extend(
        tool_calls
            .into_iter()
            .map(|call| StreamEvent::ToolCall(call.into())),
    );
    events.push(StreamEvent::Done(StreamDone {
        finish_reason,
        usage,
        metadata,
    }));
    events
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Python: tests/test_providers/test_stream_adapter.py::test_a_wrapped_response_keeps_its_thinking_signature_and_call_metadata
    // (its thinking): a response read whole reaches the loop as a stream,
    // its reasoning first; none is no event.
    #[test]
    fn a_wrapped_response_hands_its_reasoning_first() {
        let response = AiResponse {
            thinking: Some("Weighing.".to_owned()),
            ..AiResponse::new("42.")
        };
        let events = response_events(response);
        assert_eq!(
            events.first(),
            Some(&StreamEvent::ThinkingDelta(StreamThinkingDelta::new(
                "Weighing."
            )))
        );
        assert_eq!(
            events.get(1),
            Some(&StreamEvent::TextDelta(StreamTextDelta::new("42.")))
        );
        let empty = AiResponse {
            thinking: Some(String::new()),
            ..AiResponse::new("42.")
        };
        assert!(
            !response_events(empty)
                .iter()
                .any(|event| matches!(event, StreamEvent::ThinkingDelta(_)))
        );
    }

    // Python: tests/test_providers/test_stream_adapter.py::test_a_wrapped_response_keeps_its_thinking_signature_and_call_metadata
    // (its calls and its end): everything a whole response returned reaches
    // the stream, a call with its metadata, the end with its reason, its
    // usage and its metadata.
    #[test]
    fn a_wrapped_response_keeps_its_calls_and_its_end() {
        let mut call = AiToolCall::new("c1", "lookup");
        call.metadata.insert("signature".into(), json!("SIG"));
        let mut response = AiResponse {
            finish_reason: Some("tool_calls".to_owned()),
            tool_calls: vec![call.clone()],
            ..AiResponse::new("")
        };
        response.usage.insert("input_tokens".into(), json!(12));
        response.metadata.insert("model".into(), json!("model-1"));
        assert_eq!(
            response_events(response.clone()),
            [
                StreamEvent::ToolCall(stream_call_of(&call)),
                StreamEvent::Done(StreamDone {
                    finish_reason: response.finish_reason,
                    usage: response.usage,
                    metadata: response.metadata,
                }),
            ]
        );
    }

    // Python: tests/test_providers/test_thinking_blocks.py::test_a_wrapped_response_hands_each_block_on
    #[test]
    fn a_wrapped_response_hands_each_block_on() {
        let response = AiResponse {
            thinking_parts: Some(vec![
                AiThinkingPart {
                    signature: Some("S0".to_owned()),
                    ..AiThinkingPart::new("a")
                },
                AiThinkingPart {
                    redacted: Some("RRR".to_owned()),
                    ..AiThinkingPart::new("")
                },
            ]),
            ..AiResponse::new("")
        };
        let events = response_events(response);
        assert_eq!(
            events.first(),
            Some(&StreamEvent::ThinkingDelta(StreamThinkingDelta {
                block: Some(0),
                signature: Some("S0".to_owned()),
                ..StreamThinkingDelta::new("a")
            }))
        );
        assert_eq!(
            events.get(1),
            Some(&StreamEvent::ThinkingDelta(StreamThinkingDelta {
                block: Some(1),
                redacted: Some("RRR".to_owned()),
                ..StreamThinkingDelta::new("")
            }))
        );
    }

    // Python: tests/test_providers/test_stream_adapter.py::test_a_wrapped_response_keeps_its_thinking_signature_and_call_metadata
    // (its signature, a signature alone too)
    #[test]
    fn a_response_with_one_block_hands_its_signature_with_it() {
        let response = AiResponse {
            thinking: Some("Look.".to_owned()),
            thinking_signature: Some("SIG".to_owned()),
            ..AiResponse::new("")
        };
        assert_eq!(
            thinking_parts_of(&response),
            [AiThinkingPart {
                signature: Some("SIG".to_owned()),
                ..AiThinkingPart::new("Look.")
            }]
        );
        let alone = AiResponse {
            thinking_signature: Some("SIG".to_owned()),
            ..AiResponse::new("")
        };
        assert_eq!(
            response_events(alone).first(),
            Some(&StreamEvent::ThinkingDelta(StreamThinkingDelta {
                signature: Some("SIG".to_owned()),
                ..StreamThinkingDelta::new("")
            }))
        );
    }

    // Python: AIResponse carries `tasks` and `observations` for its host;
    // the contract's response reads the rest.
    #[test]
    fn a_response_read_from_python_s_json_leaves_the_host_s_lists_out() {
        let read: AiResponse = serde_json::from_value(json!({
            "content": "answer",
            "tasks": [],
            "observations": [],
        }))
        .unwrap();
        assert_eq!(read, AiResponse::new("answer"));
    }
}
