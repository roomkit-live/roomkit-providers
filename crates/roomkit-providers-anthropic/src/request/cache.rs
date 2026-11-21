//! The marks of Anthropic's prompt caching on the stable prefix of a
//! request: Python's `_apply_cache_control`. Four marks at most, as the API
//! allows: the tools, the system prompt, and the last two messages that
//! hold a block to mark, so that each round reads again, at the cached
//! rate, what the round before it marked.

use serde_json::{Value, json};

/// The blocks that may carry a mark: never a block of reasoning, which the
/// API refuses as one.
const MARKABLE: [&str; 4] = ["text", "tool_result", "tool_use", "image"];

/// How many messages are marked.
const MARKED_MESSAGES: usize = 2;

/// Writes `value` as the member `key` of `object`, when it is an object.
pub(super) fn put(object: &mut Value, key: &str, value: Value) {
    if let Some(members) = object.as_object_mut() {
        members.insert(key.to_owned(), value);
    }
}

/// Marks the last of `tools` the prefix holds, a tool held unseen being out
/// of it; `system`, which becomes a block to carry its mark; and the last
/// two of `messages` that hold a block to mark, each on its last such
/// block. A text becomes a block to carry its mark; an empty one, and a
/// message of reasoning alone, are passed over.
pub(super) fn mark(
    tools: Option<&mut [Value]>,
    system: Option<&mut Value>,
    messages: &mut [Value],
) {
    let marker = || json!({"type": "ephemeral"});
    let shown = tools.into_iter().flatten();
    let mut shown = shown.filter(|tool| tool.get("defer_loading").is_none());
    if let Some(last) = shown.next_back() {
        put(last, "cache_control", marker());
    }
    if let Some(system) = system
        && let Some(text) = system.as_str().filter(|text| !text.is_empty())
    {
        *system = json!([{"type": "text", "text": text, "cache_control": marker()}]);
    }
    let mut count = 0;
    for message in messages.iter_mut().rev() {
        if count == MARKED_MESSAGES {
            break;
        }
        let content = message.get_mut("content");
        let held = content.and_then(|content| mark_content(content, marker()));
        count += usize::from(held.is_some());
    }
}

/// Marks `content`, the content of one message: `Some` when it held a
/// block to mark.
fn mark_content(content: &mut Value, marker: Value) -> Option<()> {
    if let Some(text) = content.as_str() {
        if text.is_empty() {
            return None;
        }
        *content = json!([{"type": "text", "text": text, "cache_control": marker}]);
        return Some(());
    }
    let markable = |block: &&mut Value| {
        let kind = block.get("type").and_then(Value::as_str);
        kind.is_some_and(|kind| MARKABLE.contains(&kind))
    };
    let last = content.as_array_mut()?.iter_mut().rev().find(markable)?;
    put(last, "cache_control", marker);
    Some(())
}
