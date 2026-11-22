//! The messages of a request as Anthropic's content blocks: Python's
//! `build_messages` and `format_content`. A text, an image, a call, the
//! result of a call and a block of reasoning each become the block the
//! Messages API reads.

use roomkit_ai::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRole, AiThinkingPart,
    AiToolResultPart, ProviderError, ToolResult, ToolResultPart, image_part_base64,
};
use roomkit_core::rules::transcript::{fence, identifier};
use serde_json::{Value, json};

use super::cache::put;

/// The tag a tool's result is fenced in where it follows its references.
const RESULT_TAG: &str = "tool_result";

/// `messages` as the Messages API reads them. The API takes the roles
/// `user` and `assistant` only: a tool's results and a `system` message of
/// the history are a user turn, the system prompt being the request's own
/// member.
///
/// # Errors
///
/// The failure of an image that cannot be sent.
pub(super) fn messages(
    messages: &[AiMessage],
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let written = messages.iter().map(|message| {
        let role = match message.role {
            AiRole::Assistant => "assistant",
            AiRole::User | AiRole::System | AiRole::Tool => "user",
        };
        Ok(json!({"role": role, "content": content(&message.content, provider)?}))
    });
    written.collect()
}

/// A message's content: a text as it is, parts as their blocks. What must
/// follow every result of the message comes last: the result of a call
/// that makes tools callable.
fn content(content: &AiMessageContent, provider: &str) -> Result<Value, ProviderError> {
    let parts = match content {
        AiMessageContent::Text(text) => return Ok(text.as_str().into()),
        AiMessageContent::Parts(parts) => parts,
    };
    let (mut blocks, mut trailing) = (Vec::new(), Vec::new());
    for part in parts {
        match part {
            AiContentPart::Text(part) => blocks.push(json!({"type": "text", "text": part.text})),
            AiContentPart::Image(part) => blocks.push(image(part, provider)?),
            AiContentPart::ToolCall(part) => blocks.push(json!({
                "type": "tool_use",
                "id": part.id,
                "name": part.name,
                "input": part.arguments,
            })),
            AiContentPart::ToolResult(part) => {
                blocks.push(tool_result(part, provider)?);
                trailing.extend(beside_references(part, provider)?);
            }
            AiContentPart::Thinking(part) => blocks.extend(thinking(part)),
        }
    }
    blocks.extend(trailing);
    Ok(blocks.into())
}

/// A block of reasoning as Anthropic sent it: a redacted one as its opaque
/// data, any other with its signature. One without a signature is another
/// vendor's: Anthropic refuses it, and takes the round without it.
fn thinking(part: &AiThinkingPart) -> Option<Value> {
    if let Some(data) = &part.redacted {
        return Some(json!({"type": "redacted_thinking", "data": data}));
    }
    let signature = part.signature.as_deref().filter(|held| !held.is_empty())?;
    Some(json!({"type": "thinking", "thinking": part.thinking, "signature": signature}))
}

/// An image block: a remote URL as it is, an inline image as its media
/// type and its payload, read and written canonical.
fn image(part: &AiImagePart, provider: &str) -> Result<Value, ProviderError> {
    if !part.url.starts_with("data:") {
        return Ok(json!({"type": "image", "source": {"type": "url", "url": part.url}}));
    }
    let (media_type, data) = image_part_base64(part, provider)?;
    let source = json!({"type": "base64", "media_type": media_type, "data": data});
    Ok(json!({"type": "image", "source": source}))
}

/// The `tool_result` block of a call, flagged when the call failed. One
/// that makes tools callable carries their references alone: the API
/// refuses a reference beside any other content, so what the tool returned
/// follows the message's results ([`beside_references`]).
fn tool_result(part: &AiToolResultPart, provider: &str) -> Result<Value, ProviderError> {
    let content = if part.references.is_empty() {
        result_content(part, provider)?
    } else {
        let reference = |name: &String| json!({"type": "tool_reference", "tool_name": name});
        part.references.iter().map(reference).collect()
    };
    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": part.tool_call_id,
        "content": content,
    });
    if part.is_error {
        put(&mut block, "is_error", true.into());
    }
    Ok(block)
}

/// What a tool returned, as the content of a `tool_result`: a text as it
/// is, parts as text and image blocks.
fn result_content(part: &AiToolResultPart, provider: &str) -> Result<Value, ProviderError> {
    let parts = match &part.result {
        ToolResult::Text(text) => return Ok(text.as_str().into()),
        // A JSON value, which a Rust host may hand: as its text.
        ToolResult::Json(_) => return Ok(part.as_text().into()),
        ToolResult::Parts(parts) => parts,
    };
    let blocks = parts.iter().map(|part| match part {
        ToolResultPart::Text(part) => Ok(json!({"type": "text", "text": part.text})),
        ToolResultPart::Image(part) => image(part, provider),
    });
    blocks
        .collect::<Result<Vec<Value>, ProviderError>>()
        .map(Value::from)
}

/// The result of a call whose `tool_result` carries references, as the
/// blocks that follow the message's results: named after the call, and
/// each of its texts fenced, for what a tool returned is data, set apart in
/// a block it cannot close. Nothing for a result that is empty.
fn beside_references(part: &AiToolResultPart, provider: &str) -> Result<Vec<Value>, ProviderError> {
    if part.references.is_empty() {
        return Ok(Vec::new());
    }
    let label = format!("[Result of {}]", identifier(&part.name, "tool"));
    let text = |text: String| json!({"type": "text", "text": text});
    Ok(match result_content(part, provider)? {
        Value::String(body) if !body.is_empty() => {
            vec![text(format!("{label}\n{}", fence(RESULT_TAG, &body)))]
        }
        Value::Array(blocks) if !blocks.is_empty() => {
            let fenced = blocks.into_iter().map(|mut block| {
                let written = block.get("text").and_then(Value::as_str);
                if let Some(fenced) = written.map(|written| fence(RESULT_TAG, written)) {
                    put(&mut block, "text", fenced.into());
                }
                block
            });
            std::iter::once(text(label)).chain(fenced).collect()
        }
        Value::Null
        | Value::Bool(_)
        | Value::Number(_)
        | Value::String(_)
        | Value::Array(_)
        | Value::Object(_) => Vec::new(),
    })
}
