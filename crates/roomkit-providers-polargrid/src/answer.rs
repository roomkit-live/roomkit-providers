//! A whole answer read as the contract's response: polargrid-sdk's
//! `_convert_chat_completion_response`, then Python's `_response_of`.
//!
//! The SDK reads the answer into its models first: one without its `id`,
//! its `created`, its `model`, its `choices` or its `usage` (three counts),
//! a choice without its `index`, its `message` or one of the four finish
//! reasons it takes, a call without its `id` or a function with a name and
//! arguments written as text, fails, before RoomKit reads anything. Then the
//! usage is read first (an answer with no choice still billed its input),
//! the reasoning apart from the answer, the calls, and a constrained answer
//! checked when it called no tool.

use roomkit::model::Metadata;
use roomkit_ai::{AiContext, AiResponse, FinishReason, check_schema_answer};
use roomkit_providers_client::chat::think_tags::extract_think_tags;
use roomkit_providers_client::chat::{chat_finish_reason, message_tool_calls};
use serde::Deserialize;
use serde_json::Value;

use crate::failure::{self, Failure};

/// A whole answer, as the SDK reads it (`ChatCompletionResponse`).
#[derive(Deserialize)]
struct Completion {
    #[expect(dead_code, reason = "read for the SDK's check: an answer names its id")]
    id: String,
    #[expect(dead_code, reason = "read for the SDK's check: an answer is dated")]
    created: i64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

/// A choice of a whole answer (`ChatCompletionChoice`).
#[derive(Deserialize)]
struct Choice {
    #[expect(
        dead_code,
        reason = "read for the SDK's check: a choice names its index"
    )]
    index: i64,
    message: Message,
    finish_reason: Finish,
}

/// The finish reasons the SDK takes.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Finish {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

impl Finish {
    const fn word(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::ToolCalls => "tool_calls",
        }
    }
}

/// A choice's message (`ChatMessage`): its calls as the SDK reads them.
#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<Call>>,
}

/// A call (`ToolCall`): its id, and its function's name and arguments,
/// text both.
#[derive(Deserialize, serde::Serialize)]
struct Call {
    id: String,
    #[serde(rename = "type", default = "function")]
    kind: String,
    function: Function,
}

fn function() -> String {
    "function".to_owned()
}

#[derive(Deserialize, serde::Serialize)]
struct Function {
    name: String,
    arguments: String,
}

/// The usage (`TokenUsage`): its three counts.
#[derive(Deserialize)]
#[expect(clippy::struct_field_names, reason = "the wire's names")]
struct Usage {
    prompt_tokens: i64,
    completion_tokens: i64,
    #[expect(
        dead_code,
        reason = "read for the SDK's check: the usage has its total"
    )]
    total_tokens: i64,
}

/// The response of the answer `body`, for `request` asked of the model
/// `asked` by `provider`.
///
/// # Errors
///
/// A body that is no JSON, one the SDK does not read as an answer, and a
/// constrained answer that is not its document.
pub(crate) fn read(
    body: &[u8],
    request: &AiContext,
    asked: &str,
    provider: &str,
) -> Result<AiResponse, Failure> {
    let answer: Value = serde_json::from_slice(failure::past_mark(body))
        .map_err(|error| Failure::malformed(&error))?;
    let answer = Completion::deserialize(&answer).map_err(|error| failure::unread(&error))?;
    let mut usage = Metadata::new();
    usage.insert("input_tokens".into(), answer.usage.prompt_tokens.into());
    usage.insert(
        "output_tokens".into(),
        answer.usage.completion_tokens.into(),
    );
    let model = Some(answer.model).filter(|model| !model.is_empty());
    let mut metadata = Metadata::new();
    metadata.insert(
        "model".into(),
        model.unwrap_or_else(|| asked.to_owned()).into(),
    );
    let schema = request.response_schema.as_ref();
    let Some(choice) = answer.choices.into_iter().next() else {
        if let Some(schema) = schema {
            check_schema_answer("", schema, provider, None, None)?;
        }
        let mut response = AiResponse::new("");
        response.usage = usage;
        response.metadata = metadata;
        return Ok(response);
    };
    let (thinking, content) =
        extract_think_tags(choice.message.content.as_deref().unwrap_or_default());
    let finish_reason = chat_finish_reason(Some(choice.finish_reason.word()));
    let calls = choice.message.tool_calls.unwrap_or_default();
    let calls = serde_json::to_value(&calls).unwrap_or(Value::Null);
    let calls = message_tool_calls(Some(&calls), finish_reason);
    if let Some(schema) = schema.filter(|_| calls.is_empty()) {
        let filtered = finish_reason == Some(FinishReason::ContentFilter);
        let refusal = filtered.then_some("content_filter");
        check_schema_answer(&content, schema, provider, refusal, finish_reason)?;
    }
    let mut response = AiResponse::new(content);
    response.thinking = thinking;
    response.finish_reason = finish_reason;
    response.usage = usage;
    response.metadata = metadata;
    response.tool_calls = calls;
    Ok(response)
}
