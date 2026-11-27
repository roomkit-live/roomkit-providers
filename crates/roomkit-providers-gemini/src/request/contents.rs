//! A conversation as the Developer API reads it (Python's
//! `format_messages`): its messages as `contents`, a model's turns `model`,
//! every other `user`; a round of calls with their thought signatures, the
//! round's lent to a call Gemini left unsigned; the results as function
//! responses, their images in a user turn after them; and a round no call
//! of which is signed, in the current turn of a model that checks
//! signatures, as text, as the model reads a round another vendor answered.

use roomkit_ai::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRole, AiToolCallPart,
    AiToolResultPart, ProviderError, image_part_base64,
};
use roomkit_core::rules::transcript::{fence, identifier};
use serde_json::{Map, Value, json};

use super::json_line;

/// The metadata member a tool call keeps its thought signature under.
pub(crate) const SIGNATURE: &str = "thought_signature";

/// The image type a URL's image is sent as when its part names none.
const DEFAULT_IMAGE: &str = "image/jpeg";

/// `messages` as the request's `contents`; `signed_calls` says whether the
/// model refuses, in the current turn, a call without its signature.
pub(crate) fn contents(
    messages: &[AiMessage],
    signed_calls: bool,
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let mut contents = Vec::new();
    let mut as_text: Vec<&str> = Vec::new();
    let turn_start = if signed_calls {
        current_turn_start(messages)
    } else {
        messages.len()
    };
    for (index, message) in messages.iter().enumerate() {
        let parts: &[AiContentPart] = match &message.content {
            AiMessageContent::Parts(parts) => parts,
            AiMessageContent::Text(_) => &[],
        };
        let calls: Vec<&AiToolCallPart> = parts.iter().filter_map(call_of).collect();
        let results: Vec<&AiToolResultPart> = parts.iter().filter_map(result_of).collect();
        if !calls.is_empty() && index >= turn_start && round_signature(&calls).is_none() {
            as_text.extend(calls.iter().map(|call| call.id.as_str()));
            contents.push(round_as_text(parts));
        } else if !calls.is_empty() {
            contents.push(json!({"parts": model_call_parts(parts, &calls), "role": "model"}));
        } else if results
            .iter()
            .any(|result| as_text.contains(&result.tool_call_id.as_str()))
        {
            contents.push(results_as_text(&results, provider)?);
        } else if !results.is_empty() {
            contents.extend(tool_result_contents(&results, provider)?);
        } else {
            let role = if message.role == AiRole::Assistant {
                "model"
            } else {
                "user"
            };
            let parts = content_parts(&message.content, provider)?;
            contents.push(json!({"parts": parts, "role": role}));
        }
    }
    Ok(contents)
}

fn call_of(part: &AiContentPart) -> Option<&AiToolCallPart> {
    let AiContentPart::ToolCall(call) = part else {
        return None;
    };
    Some(call)
}

fn result_of(part: &AiContentPart) -> Option<&AiToolResultPart> {
    let AiContentPart::ToolResult(result) = part else {
        return None;
    };
    Some(result)
}

/// Where the current turn starts: just after the last user message that is
/// not a round's results.
fn current_turn_start(messages: &[AiMessage]) -> usize {
    let results = |message: &AiMessage| match &message.content {
        AiMessageContent::Parts(parts) => parts.iter().any(|part| result_of(part).is_some()),
        AiMessageContent::Text(_) => false,
    };
    messages
        .iter()
        .rposition(|message| message.role == AiRole::User && !results(message))
        .map_or(0, |index| index + 1)
}

/// The thought signature `call` carries, in the base64 Gemini wrote it in.
fn signature_of(call: &AiToolCallPart) -> Option<&str> {
    let signature = call.metadata.get(SIGNATURE).and_then(Value::as_str);
    signature.filter(|signature| !signature.is_empty())
}

/// The first signature a round's calls carry.
fn round_signature<'a>(calls: &[&'a AiToolCallPart]) -> Option<&'a str> {
    calls.iter().find_map(|call| signature_of(call))
}

/// A round as the model reads it in text: what it said, then each call.
fn round_as_text(parts: &[AiContentPart]) -> Value {
    let mut lines: Vec<String> = parts
        .iter()
        .filter_map(|part| {
            let AiContentPart::Text(text) = part else {
                return None;
            };
            (!text.text.trim().is_empty()).then(|| text.text.clone())
        })
        .collect();
    for call in parts.iter().filter_map(call_of) {
        let arguments = Value::Object(call.arguments.clone());
        let name = identifier(&call.name, "tool");
        lines.push(format!("I called {name}({}).", json_line(&arguments)));
    }
    json!({"parts": [{"text": lines.join("\n")}], "role": "model"})
}

/// The results of a round answered as text: each fenced, then the images
/// they carry.
fn results_as_text(results: &[&AiToolResultPart], provider: &str) -> Result<Value, ProviderError> {
    let mut lines = Vec::new();
    let mut images = Vec::new();
    for result in results {
        let (text, parts) = result.split_for_message();
        let verb = if result.is_error {
            "failed"
        } else {
            "returned"
        };
        let name = identifier(&result.name, "tool");
        lines.push(format!("{name} {verb}:\n{}", fence("tool_result", &text)));
        for image in &parts {
            images.push(image_part(image, provider)?);
        }
    }
    let mut parts = vec![json!({"text": lines.join("\n")})];
    parts.extend(images);
    Ok(json!({"parts": parts, "role": "user"}))
}

/// A model turn with calls: its text and its calls, each with its own
/// signature or the round's.
fn model_call_parts(parts: &[AiContentPart], calls: &[&AiToolCallPart]) -> Vec<Value> {
    let round = round_signature(calls);
    let mut written = Vec::new();
    for part in parts {
        match part {
            AiContentPart::Text(text) => written.push(json!({"text": text.text})),
            AiContentPart::ToolCall(call) => {
                let function = json!({"args": call.arguments, "name": call.name});
                let mut part = Map::new();
                part.insert("functionCall".to_owned(), function);
                if let Some(signature) = signature_of(call).or(round) {
                    part.insert("thoughtSignature".to_owned(), signature.into());
                }
                written.push(Value::Object(part));
            }
            AiContentPart::Image(_) | AiContentPart::ToolResult(_) | AiContentPart::Thinking(_) => {
            }
        }
    }
    written
}

/// A result as a function response's body: under `error` for a call that
/// failed, under `result` otherwise.
pub(crate) fn function_response_body(text: &str, is_error: bool) -> Value {
    if is_error {
        json!({"error": text})
    } else {
        json!({"result": text})
    }
}

/// The results of a round: the responses, then a user turn of their images.
fn tool_result_contents(
    results: &[&AiToolResultPart],
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let mut parts = Vec::new();
    let mut images = Vec::new();
    for result in results {
        let (text, carried) = result.split_for_message();
        let response = function_response_body(&text, result.is_error);
        parts.push(json!({"functionResponse": {"name": result.name, "response": response}}));
        for image in &carried {
            images.push(image_part(image, provider)?);
        }
    }
    let mut contents = vec![json!({"parts": parts, "role": "user"})];
    if !images.is_empty() {
        contents.push(json!({"parts": images, "role": "user"}));
    }
    Ok(contents)
}

/// A message's text and images as parts; any other part is left out.
fn content_parts(content: &AiMessageContent, provider: &str) -> Result<Vec<Value>, ProviderError> {
    let parts = match content {
        AiMessageContent::Text(text) => return Ok(vec![json!({"text": text})]),
        AiMessageContent::Parts(parts) => parts,
    };
    let mut written = Vec::new();
    for part in parts {
        match part {
            AiContentPart::Text(text) => written.push(json!({"text": text.text})),
            AiContentPart::Image(image) => written.push(image_part(image, provider)?),
            AiContentPart::ToolCall(_)
            | AiContentPart::ToolResult(_)
            | AiContentPart::Thinking(_) => {}
        }
    }
    Ok(written)
}

/// An image: its bytes inline for a data URI, refused before the request
/// when it does not read; any other URL by reference.
fn image_part(image: &AiImagePart, provider: &str) -> Result<Value, ProviderError> {
    if image.url.starts_with("data:") {
        let (kind, data) = image_part_base64(image, provider)?;
        return Ok(json!({"inlineData": {"data": data, "mime_type": kind}}));
    }
    let kind = image.mime_type.as_deref().unwrap_or(DEFAULT_IMAGE);
    Ok(json!({"fileData": {"file_uri": image.url, "mime_type": kind}}))
}
