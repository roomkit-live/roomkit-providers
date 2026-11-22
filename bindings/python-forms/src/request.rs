//! A request: Python's `AIContext`, its messages, their parts and its tools.

use std::fmt;

use roomkit::model::{Metadata, ToolCallOutcome};
use roomkit_ai::{
    API_KEY_METADATA_KEY, AiContentPart, AiContext, AiImagePart, AiMessage, AiMessageContent,
    AiRole, AiTextPart, AiThinkingPart, AiTool, AiToolCallPart, AiToolResultPart, ToolResult,
    ToolResultPart,
};
use serde::de::{Error, Unexpected, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value, json};

use crate::Form;
use crate::text_or_list::TextOrList;

/// What a credential lent to one generation is written as: a request's
/// form never holds it.
const MASK: &str = "**********";

impl Form for AiContext {
    fn read(form: Value) -> Result<Self, serde_json::Error> {
        ContextForm::deserialize(form).map(Self::from)
    }

    /// The members Python's providers read. What a channel adds for itself
    /// (the room, what the target takes, what the answer is stamped with)
    /// is left out.
    fn written(&self) -> Value {
        json!({
            "messages": self.messages.iter().map(message).collect::<Vec<_>>(),
            "system_prompt": self.system_prompt,
            "temperature": self.temperature,
            "max_tokens": self.max_tokens,
            "thinking_budget": self.thinking_budget,
            "enable_thinking": self.enable_thinking,
            "reasoning_effort": self.reasoning_effort,
            "tools": self.tools.iter().map(tool).collect::<Vec<_>>(),
            "response_schema": self.response_schema,
            "metadata": masked(&self.metadata),
        })
    }
}

/// `metadata` with the credential it lends its generation masked, as
/// Python writes it: a text that is not empty under the key's name. An
/// empty text, or a value that is no text, is no credential and is written
/// as it is.
fn masked(metadata: &Metadata) -> Value {
    let lent = |value: &Value| value.as_str().is_some_and(|key| !key.is_empty());
    let masked = metadata.iter().map(|(key, value)| {
        let value = if key == API_KEY_METADATA_KEY && lent(value) {
            Value::String(MASK.to_owned())
        } else {
            value.clone()
        };
        (key.clone(), value)
    });
    Value::Object(masked.collect())
}

fn message(message: &AiMessage) -> Value {
    let content = match &message.content {
        AiMessageContent::Text(text) => Value::String(text.clone()),
        AiMessageContent::Parts(parts) => parts.iter().map(part).collect(),
    };
    json!({
        "role": role(message.role),
        "content": content,
        "metadata": message.metadata,
    })
}

fn role(role: AiRole) -> &'static str {
    match role {
        AiRole::System => "system",
        AiRole::User => "user",
        AiRole::Assistant => "assistant",
        AiRole::Tool => "tool",
    }
}

fn part(part: &AiContentPart) -> Value {
    match part {
        AiContentPart::Text(text) => text_part(text),
        AiContentPart::Image(image) => image_part(image),
        AiContentPart::Thinking(thinking) => thinking_part(thinking),
        AiContentPart::ToolCall(call) => json!({
            "type": "tool_call",
            "id": call.id,
            "name": call.name,
            "arguments": call.arguments,
            "metadata": call.metadata,
        }),
        AiContentPart::ToolResult(result) => json!({
            "type": "tool_result",
            "tool_call_id": result.tool_call_id,
            "name": result.name,
            "result": tool_result(&result.result),
            "is_error": result.is_error,
            "structured_content": result.structured_content,
            "outcome": result.outcome,
            "references": result.references,
        }),
    }
}

fn text_part(text: &AiTextPart) -> Value {
    json!({ "type": "text", "text": text.text })
}

fn image_part(image: &AiImagePart) -> Value {
    json!({ "type": "image", "url": image.url, "mime_type": image.mime_type })
}

/// A part of reasoning, as a message and an answer both hold it.
pub(crate) fn thinking_part(thinking: &AiThinkingPart) -> Value {
    json!({
        "type": "thinking",
        "thinking": thinking.thinking,
        "signature": thinking.signature,
        "redacted": thinking.redacted,
    })
}

/// What a tool answered: a text, or its parts. A JSON value, which Python's
/// model does not hold, is written as the text of its JSON.
fn tool_result(result: &ToolResult) -> Value {
    match result {
        ToolResult::Text(text) => Value::String(text.clone()),
        ToolResult::Json(value) => Value::String(value.to_string()),
        ToolResult::Parts(parts) => parts
            .iter()
            .map(|part| match part {
                ToolResultPart::Text(text) => text_part(text),
                ToolResultPart::Image(image) => image_part(image),
            })
            .collect(),
    }
}

fn tool(tool: &AiTool) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
        "tags": tool.tags,
        "defer_loading": tool.defer_loading,
        "changes_state": tool.changes_state,
    })
}

/// The temperature of a request that states none.
fn usual_temperature() -> f64 {
    AiContext::default().temperature
}

/// [`AiContext`] as it is read.
#[derive(Deserialize)]
struct ContextForm {
    #[serde(default)]
    messages: Vec<MessageForm>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default = "usual_temperature")]
    temperature: f64,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    thinking_budget: Option<i64>,
    #[serde(default)]
    enable_thinking: Option<bool>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    tools: Vec<ToolForm>,
    #[serde(default)]
    response_schema: Option<Map<String, Value>>,
    #[serde(default)]
    metadata: Metadata,
}

impl From<ContextForm> for AiContext {
    fn from(form: ContextForm) -> Self {
        Self {
            messages: form.messages.into_iter().map(AiMessage::from).collect(),
            system_prompt: form.system_prompt,
            temperature: form.temperature,
            max_tokens: form.max_tokens,
            thinking_budget: form.thinking_budget,
            enable_thinking: form.enable_thinking,
            reasoning_effort: form.reasoning_effort,
            tools: form.tools.into_iter().map(AiTool::from).collect(),
            response_schema: form.response_schema,
            metadata: form.metadata,
            ..Self::default()
        }
    }
}

/// [`AiMessage`] as it is read.
#[derive(Deserialize)]
struct MessageForm {
    role: RoleForm,
    #[serde(deserialize_with = "content")]
    content: AiMessageContent,
    #[serde(default)]
    metadata: Metadata,
}

impl From<MessageForm> for AiMessage {
    fn from(form: MessageForm) -> Self {
        Self {
            role: form.role.into(),
            content: form.content,
            metadata: form.metadata,
        }
    }
}

/// [`AiRole`] as it is read: from its name, a text.
struct RoleForm(AiRole);

impl From<RoleForm> for AiRole {
    fn from(form: RoleForm) -> Self {
        form.0
    }
}

impl<'de> Deserialize<'de> for RoleForm {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(RoleVisitor)
    }
}

struct RoleVisitor;

impl Visitor<'_> for RoleVisitor {
    type Value = RoleForm;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one of the roles `system`, `user`, `assistant` and `tool`")
    }

    fn visit_str<E: Error>(self, name: &str) -> Result<RoleForm, E> {
        let named = [
            AiRole::System,
            AiRole::User,
            AiRole::Assistant,
            AiRole::Tool,
        ];
        let found = named.into_iter().find(|named| role(*named) == name);
        let found = found.ok_or_else(|| E::invalid_value(Unexpected::Str(name), &self))?;
        Ok(RoleForm(found))
    }
}

/// A message's content: a text, or its parts.
fn content<'de, D: Deserializer<'de>>(deserializer: D) -> Result<AiMessageContent, D::Error> {
    let expecting = "a message's content, a text or a list of parts";
    Ok(
        match TextOrList::<PartForm>::deserialize(deserializer, expecting)? {
            TextOrList::Text(text) => AiMessageContent::Text(text),
            TextOrList::List(parts) => {
                AiMessageContent::Parts(parts.into_iter().map(AiContentPart::from).collect())
            }
        },
    )
}

/// [`AiContentPart`] as it is read: the part its `type` names.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PartForm {
    Text(TextForm),
    Image(ImageForm),
    Thinking(ThinkingForm),
    ToolCall {
        id: String,
        name: String,
        #[serde(default)]
        arguments: Metadata,
        #[serde(default)]
        metadata: Metadata,
    },
    ToolResult {
        tool_call_id: String,
        name: String,
        #[serde(deserialize_with = "result")]
        result: ToolResult,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        structured_content: Option<Metadata>,
        #[serde(default)]
        outcome: Option<ToolCallOutcome>,
        #[serde(default)]
        references: Vec<String>,
    },
}

impl From<PartForm> for AiContentPart {
    fn from(form: PartForm) -> Self {
        match form {
            PartForm::Text(text) => Self::Text(text.into()),
            PartForm::Image(image) => Self::Image(image.into()),
            PartForm::Thinking(thinking) => Self::Thinking(thinking.into()),
            PartForm::ToolCall {
                id,
                name,
                arguments,
                metadata,
            } => Self::ToolCall(AiToolCallPart {
                id,
                name,
                arguments,
                metadata,
            }),
            PartForm::ToolResult {
                tool_call_id,
                name,
                result,
                is_error,
                structured_content,
                outcome,
                references,
            } => Self::ToolResult(AiToolResultPart {
                tool_call_id,
                name,
                result,
                is_error,
                structured_content,
                outcome,
                references,
            }),
        }
    }
}

/// [`AiTextPart`] as it is read.
#[derive(Deserialize)]
struct TextForm {
    text: String,
}

impl From<TextForm> for AiTextPart {
    fn from(form: TextForm) -> Self {
        Self { text: form.text }
    }
}

/// [`AiImagePart`] as it is read.
#[derive(Deserialize)]
struct ImageForm {
    url: String,
    #[serde(default)]
    mime_type: Option<String>,
}

impl From<ImageForm> for AiImagePart {
    fn from(form: ImageForm) -> Self {
        Self {
            url: form.url,
            mime_type: form.mime_type,
        }
    }
}

/// [`AiThinkingPart`] as it is read, in a message and in an answer.
#[derive(Deserialize)]
pub(crate) struct ThinkingForm {
    thinking: String,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    redacted: Option<String>,
}

impl From<ThinkingForm> for AiThinkingPart {
    fn from(form: ThinkingForm) -> Self {
        Self {
            thinking: form.thinking,
            signature: form.signature,
            redacted: form.redacted,
        }
    }
}

/// What a tool answered: a text, or its parts.
fn result<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ToolResult, D::Error> {
    let expecting = "a tool's result, a text or a list of parts";
    Ok(
        match TextOrList::<ResultPartForm>::deserialize(deserializer, expecting)? {
            TextOrList::Text(text) => ToolResult::Text(text),
            TextOrList::List(parts) => {
                ToolResult::Parts(parts.into_iter().map(ToolResultPart::from).collect())
            }
        },
    )
}

/// [`ToolResultPart`] as it is read: the part its `type` names.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResultPartForm {
    Text(TextForm),
    Image(ImageForm),
}

impl From<ResultPartForm> for ToolResultPart {
    fn from(form: ResultPartForm) -> Self {
        match form {
            ResultPartForm::Text(text) => Self::Text(text.into()),
            ResultPartForm::Image(image) => Self::Image(image.into()),
        }
    }
}

/// [`AiTool`] as it is read.
#[derive(Deserialize)]
struct ToolForm {
    name: String,
    description: String,
    #[serde(default)]
    parameters: Map<String, Value>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    defer_loading: bool,
    #[serde(default)]
    changes_state: bool,
}

impl From<ToolForm> for AiTool {
    fn from(form: ToolForm) -> Self {
        Self {
            name: form.name,
            description: form.description,
            parameters: Value::Object(form.parameters),
            tags: form.tags,
            changes_state: form.changes_state,
            defer_loading: form.defer_loading,
        }
    }
}
