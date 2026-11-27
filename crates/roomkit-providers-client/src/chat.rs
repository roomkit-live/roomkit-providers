//! What the providers of a Chat Completions wire read alike, whichever SDK
//! their Python counterpart runs on: OpenAI's and the vendors on its wire,
//! and Mistral's. Python's `openai_dialect`, which they share.
//!
//! - [`call_slots`]: a streamed answer's call fragments folded into calls,
//!   one slot per call (Python's `ToolCallSlots`);
//! - [`think_tags`]: a model's reasoning inside the `<think>` block that
//!   opens its answer, or in a field beside its text (Python's
//!   `ThinkTagParser`);
//! - [`chat_finish_reason`]: how the dialect says an answer stopped, in the
//!   vocabulary of the provider contract (RFC §6.7);
//! - [`message_tool_calls`]: the calls of a whole answer's message;
//! - [`truthy`]: whether Python reads a member of an answer as true.

use roomkit_ai::{
    AiToolCall, CallGarbledOptions, CallIds, CallPartialOptions, FinishReason, call_garbled,
    call_partial, tool_arguments,
};
use serde_json::{Map, Value};

pub mod call_slots;
pub mod think_tags;

/// Whether Python reads `value` as true: an error member that is null,
/// false, zero or empty is none.
#[must_use]
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(held) => *held,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
    }
}

/// A stop reason of the dialect in the contract's vocabulary (Python's
/// `CHAT_FINISH_REASONS`): `function_call` is the word for a round of calls
/// before `tool_calls`, and `error` a generation a compatible server
/// failed. `None` for none, and for a word the dialect does not hold, which
/// the core could only misread (a server's own extension, say).
#[must_use]
pub fn chat_finish_reason(word: Option<&str>) -> Option<FinishReason> {
    match word? {
        "stop" => Some(FinishReason::Stop),
        "tool_calls" | "function_call" => Some(FinishReason::ToolCalls),
        "length" => Some(FinishReason::Length),
        "content_filter" => Some(FinishReason::ContentFilter),
        "error" => Some(FinishReason::Error),
        _ => None,
    }
}

/// The calls of a message, as the loop reads them (Python's
/// `message_tool_calls`, RFC §6.4): each with an
/// id of its own, its arguments read as text or as an object, cut or not.
/// An entry with no function is no call; a call with no name keeps an
/// empty one, for the loop to refuse.
#[must_use]
pub fn message_tool_calls(
    entries: Option<&Value>,
    finish_reason: Option<FinishReason>,
) -> Vec<AiToolCall> {
    let entries = entries
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let functions = entries.iter().filter_map(|entry| {
        let function = entry
            .get("function")
            .filter(|function| !function.is_null())?;
        Some((entry.get("id"), function))
    });
    let functions: Vec<(Option<&Value>, &Value)> = functions.collect();
    let last = functions.len().checked_sub(1);
    let mut ids = CallIds::new();
    let mut calls = Vec::new();
    for (position, (id, function)) in functions.into_iter().enumerate() {
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let empty = Value::String(String::new());
        let raw = function.get("arguments").unwrap_or(&empty);
        let last = Some(position) == last;
        let mut call = AiToolCall::new(ids.call(&id_text(id), name), name);
        call.arguments = arguments_of(raw);
        call.partial = call_partial(raw, finish_reason, CallPartialOptions { last });
        call.garbled = call_garbled(raw, finish_reason, CallGarbledOptions { last });
        calls.push(call);
    }
    calls
}

/// A call's id as its text, as Python's `str` writes one that is no text:
/// none for an id Python reads as false (null, zero, empty), a number as
/// its digits, `True`, and a list or an object as its JSON.
fn id_text(id: Option<&Value>) -> String {
    match id.filter(|id| truthy(id)) {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Bool(_)) => "True".to_owned(),
        Some(id @ (Value::Number(_) | Value::Array(_) | Value::Object(_))) => id.to_string(),
        Some(Value::Null) | None => String::new(),
    }
}

/// A call's arguments as a mapping: an object as it is, a text read, none
/// as empty, and anything else kept whole under `raw` (Python's
/// `tool_arguments`).
fn arguments_of(raw: &Value) -> Map<String, Value> {
    match raw {
        Value::Object(arguments) => arguments.clone(),
        Value::String(text) => tool_arguments(text),
        Value::Null => Map::new(),
        other @ (Value::Bool(_) | Value::Number(_) | Value::Array(_)) => {
            Map::from_iter([("raw".to_owned(), other.clone())])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: tests/test_providers/test_chat_response_reading.py::test_the_dialect_s_stop_words_end_in_the_contract_s
    #[test]
    fn the_dialect_s_stop_words_end_in_the_contract_s() {
        let read = [
            (Some("stop"), Some(FinishReason::Stop)),
            (Some("tool_calls"), Some(FinishReason::ToolCalls)),
            (Some("function_call"), Some(FinishReason::ToolCalls)),
            (Some("length"), Some(FinishReason::Length)),
            (Some("content_filter"), Some(FinishReason::ContentFilter)),
            (Some("error"), Some(FinishReason::Error)),
            (Some("eos"), None),
            (None, None),
        ];
        for (word, reason) in read {
            assert_eq!(chat_finish_reason(word), reason, "{word:?}");
        }
    }
}
