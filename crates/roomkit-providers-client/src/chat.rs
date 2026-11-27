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
//! - [`truthy`]: whether Python reads a member of an answer as true.

use roomkit_ai::FinishReason;
use serde_json::Value;

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
