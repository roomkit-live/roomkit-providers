//! An answer of `/api/chat` as the ollama SDK reads it (`ChatResponse`), a
//! whole one or a line of a stream, and what the provider reads of it
//! (Python's `OllamaAIProvider.generate` and `_stream_events`).
//!
//! The SDK reads each answer into its models before the provider sees it:
//! one without its `message`, a message without its `role`, a call without
//! its function's `name` or with arguments that are no object, or a member
//! of another kind than the model's, fails. Ollama names no call: each is
//! given an id here, unique in its answer, never sent back.

use roomkit::model::Metadata;
use roomkit_ai::{AiToolCall, CallIds, FinishReason};
use serde::Deserialize;
use serde_json::{Map, Value};

/// An answer, or a line of a stream (`ChatResponse`).
#[derive(Deserialize)]
pub(crate) struct Answer {
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a text")]
    created_at: Option<String>,
    #[serde(default)]
    pub(crate) done: Option<bool>,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a count")]
    total_duration: Option<i64>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a count")]
    load_duration: Option<i64>,
    #[serde(default)]
    prompt_eval_count: Option<i64>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a count")]
    prompt_eval_duration: Option<i64>,
    #[serde(default)]
    eval_count: Option<i64>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a count")]
    eval_duration: Option<i64>,
    pub(crate) message: Message,
}

/// The message of an answer (`Message`).
#[derive(Deserialize)]
pub(crate) struct Message {
    #[expect(
        dead_code,
        reason = "read for the SDK's check: a message names its role"
    )]
    role: String,
    #[serde(default)]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) thinking: Option<String>,
    #[serde(default)]
    #[expect(dead_code, reason = "read for the SDK's check: a text")]
    tool_name: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<Call>>,
}

/// A call of a message (`Message.ToolCall`).
#[derive(Deserialize)]
struct Call {
    function: Function,
}

/// A call's function: its name and its arguments, an object.
#[derive(Deserialize)]
struct Function {
    name: String,
    arguments: Map<String, Value>,
}

impl Answer {
    /// How the answer ended, in the contract's word (RFC §6.7): a turn that
    /// ends on calls is done with `stop`; `load` and `unload` end no
    /// generation.
    pub(crate) fn finish_reason(&self) -> Option<FinishReason> {
        match self.done_reason.as_deref()? {
            "stop" => Some(FinishReason::Stop),
            "length" => Some(FinishReason::Length),
            _ => None,
        }
    }

    /// The counts the answer reports, each where it reports one.
    pub(crate) fn usage(&self) -> Metadata {
        let mut usage = Metadata::new();
        if let Some(prompt) = self.prompt_eval_count {
            usage.insert("input_tokens".into(), prompt.into());
        }
        if let Some(output) = self.eval_count {
            usage.insert("output_tokens".into(), output.into());
        }
        usage
    }

    /// The model that answered, else the one asked.
    pub(crate) fn answered_by(&self) -> Option<&str> {
        self.model.as_deref().filter(|model| !model.is_empty())
    }
}

impl Message {
    /// The calls the message carries, each given an id by `ids`.
    pub(crate) fn calls(&self, ids: &mut CallIds) -> Vec<AiToolCall> {
        let calls = self.tool_calls.iter().flatten();
        let calls = calls.map(|call| {
            let name = call.function.name.as_str();
            let mut read = AiToolCall::new(ids.call("", name), name);
            read.arguments.clone_from(&call.function.arguments);
            read
        });
        calls.collect()
    }
}
