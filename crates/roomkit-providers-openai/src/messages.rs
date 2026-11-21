//! A conversation rendered as Chat Completions messages: Python's
//! `chat_messages`, with `OpenAI`'s own rendering and the field a
//! configuration names for a model's earlier reasoning.

use roomkit_providers_core::{
    AiContentPart, AiImagePart, AiMessage, AiMessageContent, AiRole, AiToolCallPart,
    AiToolResultPart, ProviderError, dumps, image_part_uri,
};
use serde_json::{Map, Value, json};

/// The conversation as the endpoint reads it, the system prompt first.
/// `thinking_field` is the assistant message field a server reads a model's
/// earlier reasoning from; `None` sends none (RFC §6.4).
///
/// # Errors
///
/// The failure of an image the provider cannot send.
pub(crate) fn chat_messages(
    messages: &[AiMessage],
    system_prompt: Option<&str>,
    thinking_field: Option<&str>,
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let mut rendered = Vec::new();
    if let Some(prompt) = system_prompt.filter(|prompt| !prompt.is_empty()) {
        rendered.push(json!({"role": "system", "content": prompt}));
    }
    for message in messages {
        match &message.content {
            AiMessageContent::Text(text) => {
                rendered.push(json!({"role": role(message.role), "content": text}));
            }
            AiMessageContent::Parts(parts) => {
                rendered.extend(of_parts(message.role, parts, thinking_field, provider)?);
            }
        }
    }
    Ok(rendered)
}

fn role(role: AiRole) -> &'static str {
    match role {
        AiRole::System => "system",
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
        AiRole::Tool => "tool",
    }
}

/// The messages one message of parts becomes: a round that called tools, a
/// tool message per result with their images after, or its text and images.
fn of_parts(
    from: AiRole,
    parts: &[AiContentPart],
    thinking_field: Option<&str>,
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let calls: Vec<&AiToolCallPart> = parts.iter().filter_map(tool_call).collect();
    let field = thinking_field.filter(|_| from == AiRole::Assistant);
    let thinking = field.map(|field| (field, thinking_of(parts)));
    let thought = thinking.filter(|(_, thinking)| !thinking.is_empty());
    if !calls.is_empty() {
        let mut round = call_round(parts, &calls);
        if let Some((field, thinking)) = thought {
            round.insert(field.to_owned(), thinking.into());
        }
        return Ok(vec![Value::Object(round)]);
    }
    let results: Vec<&AiToolResultPart> = parts.iter().filter_map(tool_result).collect();
    if !results.is_empty() {
        return tool_messages(&results, provider);
    }
    let mut blocks = Vec::new();
    for part in parts {
        match part {
            AiContentPart::Text(text) => blocks.push(json!({"type": "text", "text": text.text})),
            AiContentPart::Image(image) => blocks.push(image_block(image, provider)?),
            AiContentPart::Thinking(_)
            | AiContentPart::ToolCall(_)
            | AiContentPart::ToolResult(_) => {}
        }
    }
    // A message whose only part is reasoning keeps an empty text.
    let content = if blocks.is_empty() && !parts.is_empty() {
        Value::String(String::new())
    } else {
        Value::Array(blocks)
    };
    let mut plain = Map::new();
    plain.insert("role".to_owned(), role(from).into());
    plain.insert("content".to_owned(), content);
    if let Some((field, thinking)) = thought {
        plain.insert(field.to_owned(), thinking.into());
    }
    Ok(vec![Value::Object(plain)])
}

fn tool_call(part: &AiContentPart) -> Option<&AiToolCallPart> {
    if let AiContentPart::ToolCall(call) = part {
        return Some(call);
    }
    None
}

fn tool_result(part: &AiContentPart) -> Option<&AiToolResultPart> {
    if let AiContentPart::ToolResult(result) = part {
        return Some(result);
    }
    None
}

/// The text of a part that is text.
fn text_of(part: &AiContentPart) -> Option<&str> {
    if let AiContentPart::Text(text) = part {
        return Some(&text.text);
    }
    None
}

/// The reasoning of a message's parts, joined.
fn thinking_of(parts: &[AiContentPart]) -> String {
    let thinking = parts.iter().filter_map(|part| {
        if let AiContentPart::Thinking(thought) = part {
            return Some(thought.thinking.as_str());
        }
        None
    });
    thinking.collect()
}

/// An assistant round that called tools, what it said beside its calls. A
/// call's arguments go as Python's `json.dumps` writes them.
fn call_round(parts: &[AiContentPart], calls: &[&AiToolCallPart]) -> Map<String, Value> {
    let said: String = parts.iter().filter_map(text_of).collect();
    let calls: Vec<Value> = calls
        .iter()
        .map(|call| {
            let arguments = dumps(&Value::Object(call.arguments.clone()));
            json!({
                "id": call.id,
                "type": "function",
                "function": {"name": call.name, "arguments": arguments},
            })
        })
        .collect();
    let mut round = Map::new();
    round.insert("role".to_owned(), "assistant".into());
    let content = if said.is_empty() {
        Value::Null
    } else {
        said.into()
    };
    round.insert("content".to_owned(), content);
    round.insert("tool_calls".to_owned(), calls.into());
    round
}

/// One tool message per result, then the results' images on a user message:
/// a tool message carries text only.
fn tool_messages(
    results: &[&AiToolResultPart],
    provider: &str,
) -> Result<Vec<Value>, ProviderError> {
    let mut rendered = Vec::new();
    let mut images = Vec::new();
    for result in results {
        let (text, shown) = result.result.split_for_message();
        rendered.push(json!({
            "role": "tool",
            "tool_call_id": result.tool_call_id,
            "content": text,
        }));
        for image in &shown {
            images.push(image_block(image, provider)?);
        }
    }
    if !images.is_empty() {
        rendered.push(json!({"role": "user", "content": images}));
    }
    Ok(rendered)
}

fn image_block(image: &AiImagePart, provider: &str) -> Result<Value, ProviderError> {
    let url = image_part_uri(image, provider)?;
    Ok(json!({"type": "image_url", "image_url": {"url": url}}))
}
