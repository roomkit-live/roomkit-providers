//! The conversation as the transcript writes it, which the Responses input
//! is rendered from.

use roomkit::model::ToolCall;
use roomkit_ai::{AiContentPart, AiMessage, AiMessageContent, AiRole};
use roomkit_core::rules::transcript::{DataUriError, Image, Message, Part, Role, parse_data_uri};
use roomkit_providers_client::failure::own;

use crate::failure::Failure;

/// The transcript messages `message` is: a round's tool message is one per
/// result, each answering its call with its text, then the results' images
/// on a user message: a tool message carries text only, an image part is a
/// user's (Python's `chat_request._tool_messages`); any other is one. A
/// message that calls is the round that called, whatever results it also
/// holds, which are left out (Python's `chat_request._rendered`).
pub(super) fn response_messages(message: &AiMessage) -> Result<Vec<Message>, Failure> {
    let parts = match &message.content {
        AiMessageContent::Parts(parts) => parts.as_slice(),
        AiMessageContent::Text(_) => &[],
    };
    if parts
        .iter()
        .any(|part| matches!(part, AiContentPart::ToolCall(_)))
    {
        return Ok(vec![self::message(message)?]);
    }
    let results: Vec<_> = match &message.content {
        AiMessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                AiContentPart::ToolResult(result) => Some(result),
                AiContentPart::Text(_)
                | AiContentPart::Image(_)
                | AiContentPart::Thinking(_)
                | AiContentPart::ToolCall(_) => None,
            })
            .collect(),
        AiMessageContent::Text(_) => Vec::new(),
    };
    if results.is_empty() {
        return Ok(vec![self::message(message)?]);
    }
    let mut written = Vec::with_capacity(results.len() + 1);
    let mut images = Vec::new();
    for result in results {
        let (text, result_images) = result.split_for_message();
        let mut answer = Message::tool_result(&result.tool_call_id, text);
        answer.role = Role::Tool;
        answer.metadata.clone_from(&message.metadata);
        written.push(answer);
        for image in result_images {
            let mime_type = image.mime_type.as_deref().unwrap_or_default();
            images.extend(read_image(&image.url, mime_type)?);
        }
    }
    if !images.is_empty() {
        let mut shown = Message::new(Role::User, "");
        shown.parts = images.iter().cloned().map(Part::Image).collect();
        shown.images = images;
        written.push(shown);
    }
    Ok(written)
}

/// Normalize a source message into the transcript layout used by Responses.
/// Reasoning items for that API are replayed from the provider's response metadata.
fn message(message: &AiMessage) -> Result<Message, Failure> {
    let parts: &[AiContentPart] = match &message.content {
        AiMessageContent::Text(text) => {
            let mut written = Message::new(role(message.role), text.clone());
            written.metadata.clone_from(&message.metadata);
            return Ok(written);
        }
        AiMessageContent::Parts(parts) => parts,
    };
    let mut text = String::new();
    let mut images = Vec::new();
    let mut ordered = Vec::new();
    let mut calls = Vec::new();
    for part in parts {
        match part {
            AiContentPart::Text(part) => {
                text.push_str(&part.text);
                ordered.push(Part::Text(part.text.clone()));
            }
            AiContentPart::Image(image) => {
                let mime_type = image.mime_type.as_deref().unwrap_or_default();
                if let Some(image) = read_image(&image.url, mime_type)? {
                    ordered.push(Part::Image(image.clone()));
                    images.push(image);
                }
            }
            AiContentPart::ToolCall(call) => {
                calls.push(ToolCall::new(&call.id, &call.name, call.arguments.clone()));
            }
            // A message of results alone is written by `response_messages`;
            // a round that called leaves its results out.
            AiContentPart::ToolResult(_) | AiContentPart::Thinking(_) => {}
        }
    }
    let plain = calls.is_empty();
    let mut written = Message::assistant_calls(text, calls);
    if plain {
        written.parts = ordered;
        written.role = role(message.role);
    }
    written.images = images;
    written.metadata.clone_from(&message.metadata);
    Ok(written)
}

/// The image a part names, as Python's `image_part_uri` forwards it: a
/// link as it is, a `data:` URI read and rebuilt from its bytes, so that
/// whitespace and a missing padding are repaired and only the type stays
/// of its header; `None` for one the transcript does not take.
///
/// # Errors
///
/// The failure of a `data:` URI that does not read (no payload, no base64,
/// the URL-safe alphabet's included): the request is never sent.
fn read_image(url: &str, mime_type: &str) -> Result<Option<Image>, Failure> {
    if !url.starts_with("data:") {
        return Ok(Image::new(url, mime_type));
    }
    let fallback = Some(mime_type).filter(|kind| !kind.is_empty());
    let (kind, data) = parse_data_uri(url, fallback).map_err(invalid_image)?;
    Ok(Image::from_bytes(&data, &kind))
}

/// The failure of an image part that does not read: final, in the words
/// every provider says it in.
fn invalid_image(cause: DataUriError) -> Failure {
    let said = format!("invalid image part: {cause}");
    Failure::said(vec![own(said)], false, None)
}

/// The transcript's role of `role`: each keeps its own, the system's
/// included, as Python sends `message.role` (`chat_request._rendered`).
fn role(role: AiRole) -> Role {
    match role {
        AiRole::Assistant => Role::Assistant,
        AiRole::Tool => Role::Tool,
        AiRole::User => Role::User,
        AiRole::System => Role::System,
    }
}
