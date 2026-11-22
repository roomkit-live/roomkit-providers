//! The Responses wire: the body of a request, and a terminal response read
//! whole.

use roomkit::model::Metadata;
use roomkit_ai::{AiContext, AiMessage, FinishReason, StreamDone};
use roomkit_providers_client::failure::{
    MESSAGE_LIMIT, described_status, names_transient, retryable_status, told,
};
use serde_json::{Map, Value, json};

use super::replay::{OUTPUT, append_message};
use super::transcript::response_messages;
use crate::call_slots::RawCall;
use crate::config::OpenAiConfig;
use crate::failure::Failure;

/// A whole answer as the wire read it: its reasoning, its text, its calls,
/// its end.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Whole {
    pub(super) thinking: Option<String>,
    pub(super) text: String,
    pub(super) calls: Vec<RawCall>,
    pub(super) done: StreamDone,
}

/// The failure of an answer that is not the Responses wire's: final.
pub(super) fn malformed(reason: &'static str) -> Failure {
    Failure::unreadable(reason)
}

/// The failure of a request the provider cannot write: final, and nothing
/// is sent.
pub(super) fn unwritable(reason: &'static str) -> Failure {
    Failure::before_request(reason)
}

/// Render the conversation into Responses input, preserving replay metadata.
pub(crate) fn input(messages: &[AiMessage]) -> Result<Vec<Value>, Failure> {
    // The provider validates every input image before it reads stored Responses output.
    let mut normalized = Vec::new();
    for source in messages {
        normalized.extend(response_messages(source)?);
    }
    let mut input = Vec::new();
    for message in &normalized {
        append_message(&mut input, message)?;
    }
    Ok(input)
}

/// Build the Responses body of `request` using its rendered `input`, its
/// answer constrained to `schema` when the request carries one.
pub(crate) fn request(
    config: &OpenAiConfig,
    request: &AiContext,
    mut input: Vec<Value>,
    schema: Option<&Map<String, Value>>,
) -> Result<Map<String, Value>, Failure> {
    let tools: Vec<_> = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function", "name": tool.name, "description": tool.description,
                "parameters": tool.parameters, "strict": false,
            })
        })
        .collect();
    let tools = if let Some((namespace, description)) = &config.tool_namespace {
        if namespace.is_empty()
            || namespace.len() > 64
            || !namespace
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(unwritable("invalid Responses tool namespace"));
        }
        for item in &mut input {
            if item.get("type").and_then(Value::as_str) == Some("function_call")
                && let Some(fields) = item.as_object_mut()
            {
                fields.insert("namespace".into(), namespace.clone().into());
            }
        }
        if tools.is_empty() {
            Vec::new()
        } else {
            vec![
                json!({"type":"namespace", "name":namespace, "description":description, "tools":tools}),
            ]
        }
    } else {
        tools
    };
    let mut body = Map::new();
    body.insert("model".into(), config.model.as_str().into());
    body.insert("input".into(), input.into());
    body.insert("stream".into(), true.into());
    body.insert("store".into(), false.into());
    body.insert("include".into(), json!(["reasoning.encrypted_content"]));
    body.insert("tools".into(), tools.into());
    // A response schema constrains the answer's text strictly, as OpenAI
    // documents it for this API (RFC §6.7).
    if let Some(schema) = schema {
        let format = json!({ "type": "json_schema", "name": "response", "schema": schema,
            "strict": true });
        body.insert("text".into(), json!({ "format": format }));
    }
    // The reasoning effort is the provider's to write (`Asked::responses_body`).
    if let Some(prompt) = &request.system_prompt {
        body.insert("instructions".into(), prompt.clone().into());
    }
    Ok(body)
}

/// Why `item` is no reasoning item a stateless request can send back.
pub(super) fn validate_reasoning(item: &Value) -> Result<(), &'static str> {
    if item.get("type").and_then(Value::as_str) != Some("reasoning")
        || item
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err("stateless reasoning replay requires an encrypted reasoning item");
    }
    Ok(())
}

/// The failure a failed or cancelled response names: its code and its
/// message. The status it names decides whether it is retried, else its
/// words.
pub(super) fn failed(value: &Value) -> Failure {
    let error = value.get("error").filter(|v| !v.is_null()).unwrap_or(value);
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("provider_error");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("Responses generation failed");
    let said = format!("{code}: {message}");
    let status = described_status(error);
    let retryable = status.map_or_else(|| names_transient(&said), retryable_status);
    Failure::said(vec![told(said, MESSAGE_LIMIT)], retryable, status)
}

/// A JSON response answered in place of its stream, read as a terminal
/// event's is.
pub(super) fn body(value: &Value, namespace: Option<&str>) -> Result<Whole, Failure> {
    validate_namespace(value, namespace)?;
    response(value)
}

/// Why an incomplete response stopped, in the contract's vocabulary (RFC
/// §6.7): its output cap, as one that gives no reason; a filter's stop; and
/// no reason for a word the contract does not hold.
fn incomplete(reason: Option<&str>) -> Option<FinishReason> {
    match reason {
        Some("max_output_tokens") | None => Some(FinishReason::Length),
        Some("content_filter") => Some(FinishReason::ContentFilter),
        Some(_) => None,
    }
}

/// Only a terminal response can release tool calls to the engine.
pub(super) fn response(value: &Value) -> Result<Whole, Failure> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("Responses answer has no status"))?;
    if matches!(status, "failed" | "cancelled") {
        return Err(failed(value));
    }
    if !matches!(status, "completed" | "incomplete") {
        return Err(malformed("Responses answer is not terminal"));
    }
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("Responses answer has no output array"))?;
    let mut text = String::new();
    let mut refusal = String::new();
    let mut thinking = String::new();
    let mut calls = Vec::new();
    let mut replay = Vec::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("message") => message_text(item, &mut text, &mut refusal).map_err(malformed)?,
            Some("function_call") => {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| malformed("function call has no name"))?;
                let id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = match item.get("arguments") {
                    Some(Value::String(text)) => text.clone(),
                    None | Some(Value::Null) => String::new(),
                    Some(value) => value.to_string(),
                };
                calls.push(RawCall::new(id, name, arguments));
            }
            Some("reasoning") => {
                reasoning_text(item, &mut thinking);
                // A cut reasoning item is not replayable. A complete item needs
                // its opaque payload; an id alone cannot be resolved with store=false.
                if status == "incomplete"
                    && item.get("encrypted_content").is_none_or(Value::is_null)
                {
                    continue;
                }
                validate_reasoning(item).map_err(malformed)?;
            }
            _ => return Err(malformed("unsupported Responses output item")),
        }
        replay.push(item.clone());
    }
    let finish_reason = if status == "incomplete" {
        let reason = value.pointer("/incomplete_details/reason");
        incomplete(reason.and_then(Value::as_str))
    } else if calls.is_empty() {
        Some(FinishReason::Stop)
    } else {
        Some(FinishReason::ToolCalls)
    };
    let mut done = StreamDone {
        finish_reason,
        usage: usage(value.get("usage")),
        metadata: Metadata::new(),
    };
    if let Some(model) = value.get("model").and_then(Value::as_str) {
        done.metadata.insert("model".into(), model.into());
    }
    // A refusal stays the answer's text, and the end names it, as Chat's
    // `refusal` does: a constrained answer then fails `refusal`.
    if !refusal.is_empty() {
        done.metadata.insert("refusal".into(), refusal.into());
    }
    done.metadata.insert(OUTPUT.into(), replay.into());
    let thinking = Some(thinking).filter(|thinking| !thinking.is_empty());
    Ok(Whole {
        thinking,
        text,
        calls,
        done,
    })
}

/// Adds to `thinking` what a reasoning item shows of the model's reasoning:
/// its `summary` parts and its `content` parts, in order. Most items show
/// none, the reasoning itself in their opaque payload.
fn reasoning_text(item: &Value, thinking: &mut String) {
    for member in ["summary", "content"] {
        let parts = item.get(member).and_then(Value::as_array);
        for part in parts.into_iter().flatten() {
            let read = part.get("text").and_then(Value::as_str);
            thinking.push_str(read.unwrap_or_default());
        }
    }
}

/// Adds to `text` what an output `message` says, its refusals included,
/// which `refusal` also gathers; or why the message does not read.
pub(super) fn message_text(
    message: &Value,
    text: &mut String,
    refusal: &mut String,
) -> Result<(), &'static str> {
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err("Responses output message has an invalid role");
    }
    let parts = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or("Responses output message has no content array")?;
    for part in parts {
        let field = match part.get("type").and_then(Value::as_str) {
            Some("output_text") => "text",
            Some("refusal") => "refusal",
            _ => return Err("unsupported Responses output content"),
        };
        let said = part
            .get(field)
            .and_then(Value::as_str)
            .ok_or("Responses text part has no text")?;
        text.push_str(said);
        if field == "refusal" {
            refusal.push_str(said);
        }
    }
    Ok(())
}

fn usage(value: Option<&Value>) -> Metadata {
    let mut counted = Metadata::new();
    let Some(value) = value else {
        return counted;
    };
    let cached = value
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    if let Some(input) = value.get("input_tokens").and_then(Value::as_u64) {
        counted.insert("input_tokens".into(), input.saturating_sub(cached).into());
        if cached > 0 {
            counted.insert("cache_read_input_tokens".into(), cached.min(input).into());
        }
    }
    if let Some(output) = value.get("output_tokens").and_then(Value::as_u64) {
        counted.insert("output_tokens".into(), output.into());
    }
    if let Some(reasoning) = value
        .pointer("/output_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64)
    {
        counted.insert("reasoning_tokens".into(), reasoning.into());
    }
    counted
}

/// Namespace mismatches fail before a terminal snapshot releases any calls.
pub(super) fn validate_namespace(value: &Value, expected: Option<&str>) -> Result<(), Failure> {
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            if item.get("type").and_then(Value::as_str) == Some("function_call")
                && (item
                    .get("namespace")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
                    || item
                        .get("namespace")
                        .and_then(Value::as_str)
                        .is_some_and(|namespace| Some(namespace) != expected))
            {
                return Err(malformed(
                    "Responses function call has an unexpected namespace",
                ));
            }
        }
    }
    Ok(())
}
