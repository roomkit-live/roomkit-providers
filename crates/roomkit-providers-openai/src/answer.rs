//! A whole answer read as the contract's response: Python's `_response_of`
//! and the readers of the Chat Completions dialect it calls.

use roomkit_providers_core::{
    AiRequest, AiResponse, AiToolCall, CallGarbledOptions, CallIds, CallPartialOptions, Metadata,
    ProviderError, call_garbled, call_partial, check_schema_answer, tool_arguments,
};
use serde_json::{Map, Value};

use crate::failure::Failing;

/// The tag that opens a model's reasoning at the start of its text, and
/// the one that closes it.
const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

/// What may stand before the block that opens a generation: spaces, tabs
/// and line breaks, which stay the answer's (RFC §6.7).
const LEADING_SPACE: [char; 4] = [' ', '\t', '\r', '\n'];

/// The response of a 2xx answer: `body` as it came, `json` whether the
/// server said it is JSON, for `request` asked of the model `asked`.
///
/// # Errors
///
/// The failure of a body that is no completion, of an error object a
/// gateway answered with, and of a constrained answer that is not its
/// document.
pub(crate) fn read(
    body: &[u8],
    json: bool,
    request: &AiRequest,
    asked: &str,
    failing: Failing<'_>,
) -> Result<AiResponse, ProviderError> {
    let no_completion = || failing.unreadable("not a chat completion");
    if !json {
        return Err(no_completion());
    }
    let answer: Value = serde_json::from_slice(body).map_err(|error| failing.malformed(&error))?;
    let completion = answer.as_object().ok_or_else(no_completion)?;
    if let Some(error) = completion.get("error").filter(|error| truthy(error)) {
        return Err(failing.error_object(error));
    }
    let usage = completion.get("usage").map(usage_of).unwrap_or_default();
    // The model that answered, as the answer names it, else the one asked.
    let model = completion.get("model").filter(|model| truthy(model));
    let mut response = AiResponse::new("");
    response.usage = usage;
    let model = model.cloned().unwrap_or_else(|| asked.into());
    response.metadata.insert("model".to_owned(), model);
    let schema = request.response_schema.as_ref();
    let choice = match completion.get("choices") {
        Some(Value::Array(choices)) => choices.first(),
        // Choices left out or null are none, as Python reads them; choices
        // that are no list are no completion.
        Some(Value::Null) | None => None,
        Some(Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => {
            return Err(no_completion());
        }
    };
    let Some(choice) = choice else {
        if let Some(schema) = schema {
            check_schema_answer("", schema, failing.provider, None, None)?;
        }
        return Ok(response);
    };
    let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
    let message = choice.get("message").filter(|message| message.is_object());
    let member = |name: &str| message.and_then(|message| message.get(name));
    let text = |name: &str| {
        member(name)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    let calls = tool_calls(member("tool_calls"), finish_reason);
    let (thinking, content) = extract_think_tags(text("content").unwrap_or_default());
    // The first of the two fields that holds anything, when it is a text.
    let held = ["reasoning_content", "reasoning"]
        .into_iter()
        .filter_map(&member);
    let field = held
        .clone()
        .find(|held| truthy(held))
        .and_then(Value::as_str);
    let refusal = text("refusal");
    if let Some(schema) = schema.filter(|_| calls.is_empty()) {
        let filtered = (finish_reason == Some("content_filter")).then_some("content_filter");
        let declined = refusal.or(filtered);
        check_schema_answer(&content, schema, failing.provider, declined, finish_reason)?;
    }
    response.content = content;
    response.thinking = merge_thinking(thinking, field);
    response.finish_reason = finish_reason.map(str::to_owned);
    if let Some(refusal) = refusal {
        response
            .metadata
            .insert("refusal".to_owned(), refusal.into());
    }
    response.tool_calls = calls;
    Ok(response)
}

/// Whether Python reads `value` as true: an error member that is null,
/// false, zero or empty is none.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(held) => *held,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
    }
}

/// The calls of a message, as the loop reads them (RFC §6.4): each with an
/// id of its own, its arguments read as text or as an object, cut or not.
/// An entry with no function is no call; a call with no name keeps an
/// empty one, for the loop to refuse.
fn tool_calls(entries: Option<&Value>, finish_reason: Option<&str>) -> Vec<AiToolCall> {
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
fn arguments_of(raw: &Value) -> Metadata {
    match raw {
        Value::Object(arguments) => arguments.clone(),
        Value::String(text) => tool_arguments(text),
        Value::Null => Map::new(),
        other @ (Value::Bool(_) | Value::Number(_) | Value::Array(_)) => {
            Map::from_iter([("raw".to_owned(), other.clone())])
        }
    }
}

/// The text of a whole response split into the `<think>` block that opens
/// it and its answer (RFC §6.4, §6.7; Python's `extract_think_tags`): only
/// a block that opens the text, after nothing but spaces, tabs and line
/// breaks, is reasoning; a block the response stopped before closing is
/// reasoning too, and a tag the answer writes later is answer.
fn extract_think_tags(text: &str) -> (Option<String>, String) {
    let body = text.trim_start_matches(LEADING_SPACE);
    let Some(block) = body.strip_prefix(THINK_OPEN) else {
        return (None, text.to_owned());
    };
    let leading = text.strip_suffix(body).unwrap_or_default();
    let (thinking, answer) = block.split_once(THINK_CLOSE).unwrap_or((block, ""));
    let thinking = Some(thinking).filter(|thinking| !thinking.is_empty());
    (thinking.map(str::to_owned), format!("{leading}{answer}"))
}

/// The two reasoning conventions as one trace: the block that opens the
/// text, then the field beside it.
fn merge_thinking(inline: Option<String>, field: Option<&str>) -> Option<String> {
    match (inline, field) {
        (inline, None) => inline,
        (None, Some(field)) => Some(field.to_owned()),
        (Some(inline), Some(field)) => Some(format!("{inline}{field}")),
    }
}

/// A count of tokens a server wrote as a whole number, a float with no
/// fraction included; `None` for anything else, which stays unknown.
fn count(value: Option<&Value>) -> Option<u64> {
    let number = value?.as_number()?;
    number.as_u64().or_else(|| {
        let float = number
            .as_f64()
            .filter(|float| *float >= 0.0 && float.fract() == 0.0)?;
        format!("{float:.0}").parse().ok()
    })
}

/// A server's usage as the contract's counters (Python's `_usage_from`):
/// the prompt's tokens without its cache reads and its cache writes, which
/// are counted apart; a counter the server leaves out stays unknown.
fn usage_of(raw: &Value) -> Metadata {
    let prompt = count(raw.get("prompt_tokens"));
    let completion = count(raw.get("completion_tokens"));
    let details = raw.get("prompt_tokens_details");
    let detail = |name: &str| count(details.and_then(|details| details.get(name)));
    let written = detail("cache_write_tokens")
        .filter(|written| *written > 0)
        .or_else(|| count(raw.get("cache_write_tokens")))
        .unwrap_or_default();
    let mut usage = Map::new();
    let mut cached = detail("cached_tokens").unwrap_or_default();
    if let Some(prompt) = prompt {
        cached = cached.min(prompt);
        let fresh = prompt.saturating_sub(cached).saturating_sub(written);
        usage.insert("input_tokens".to_owned(), fresh.into());
    }
    if let Some(completion) = completion {
        usage.insert("output_tokens".to_owned(), completion.into());
    }
    if cached > 0 && prompt.is_some() {
        usage.insert("cache_read_input_tokens".to_owned(), cached.into());
    }
    if written > 0 {
        usage.insert("cache_creation_input_tokens".to_owned(), written.into());
    }
    let reasoning = raw.get("completion_tokens_details");
    let reasoning = count(reasoning.and_then(|details| details.get("reasoning_tokens")));
    if let Some(reasoning) = reasoning.filter(|reasoning| *reasoning > 0) {
        usage.insert("reasoning_tokens".to_owned(), reasoning.into());
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: tests/test_providers/test_openai.py::TestExtractThinkTags and
    // ::test_where_a_think_tag_stands
    #[test]
    fn only_the_block_that_opens_a_text_is_reasoning() {
        let split = |text: &str| {
            let (thinking, answer) = extract_think_tags(text);
            (thinking.unwrap_or_default(), answer)
        };
        let read = |thinking: &str, answer: &str| (thinking.to_owned(), answer.to_owned());
        assert_eq!(split("Hello."), read("", "Hello."));
        assert_eq!(
            split("<think>A plan.</think>Hello."),
            read("A plan.", "Hello.")
        );
        assert_eq!(
            split("<think>line\nline</think>\nHello."),
            read("line\nline", "\nHello.")
        );
        assert_eq!(split("<think></think>Hello."), read("", "Hello."));
        assert_eq!(split(" \t\r\n<think>A</think>B"), read("A", " \t\r\nB"));
        assert_eq!(split("<think>never closed"), read("never closed", ""));
        assert_eq!(split("<think>held</thi"), read("held</thi", ""));
        assert_eq!(
            split("<think>one</think>B<think>two</think>"),
            read("one", "B<think>two</think>")
        );
        for text in [
            "Hello. <think>is text</think>",
            "<thinking>A plan.</thinking>Hello.",
            "\u{feff}<think>A</think>B",
            "\u{a0}<think>A</think>B",
            "<thi",
        ] {
            assert_eq!(split(text), read("", text), "{text}");
        }
    }

    #[test]
    fn a_count_is_a_whole_number_and_nothing_else() {
        let read = |text: &str| count(serde_json::from_str::<Value>(text).ok().as_ref());
        assert_eq!(read("12"), Some(12));
        assert_eq!(read("12.0"), Some(12));
        assert_eq!(read("0"), Some(0));
        for unknown in ["12.5", "-12", "\"12\"", "true", "null", "[12]", "1e400"] {
            assert_eq!(read(unknown), None, "{unknown}");
        }
    }
}
