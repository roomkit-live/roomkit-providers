//! What the providers of a Chat Completions wire read alike, whichever SDK
//! their Python counterpart runs on: OpenAI's and the vendors on its wire,
//! and Mistral's. Python's `openai_dialect`, which they share.
//!
//! - [`call_slots`]: a streamed answer's call fragments folded into calls,
//!   one slot per call (Python's `ToolCallSlots`);
//! - [`think_tags`]: a model's reasoning inside the `<think>` block that
//!   opens its answer, or in a field beside its text (Python's
//!   `ThinkTagParser`);
//! - [`truthy`]: whether Python reads a member of an answer as true.

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
