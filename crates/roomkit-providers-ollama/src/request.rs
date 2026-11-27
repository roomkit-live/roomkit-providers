//! A request as Ollama's `/api/chat` reads it: Python's
//! `OllamaAIProvider._build_kwargs` and `_build_messages`, its body written
//! as the ollama SDK writes it.
//!
//! Without tools, the SDK writes the body from its `ChatRequest`: its
//! members in the order the model declares them, the tools an empty list, a
//! count of seconds to keep the model loaded written as a number with a
//! fraction. With tools, `RoomKit` writes it itself (`sdk_patch.chat`), so
//! that the SDK drops nothing of a tool's schema: the members in the order
//! the provider sets them, the tools and the stream last. Either way each
//! message is the SDK's `Message`: its members in their order, an empty one
//! left out.

use roomkit_ai::{
    AiContentPart, AiContext, AiMessage, AiMessageContent, AiToolResultPart,
    chat_tool_declarations, schema_for_generate,
};
use roomkit_providers_client::reasoning::{EFFORTS, nearest_level, thinking_switch};
use serde_json::{Map, Value, json};

use crate::config::{KeepAlive, OllamaConfig, Think, keep_alive_value};
use crate::failure::Failure;
use crate::images;

/// The levels Ollama's `think` takes, on a model that takes any (gpt-oss).
const THINK_LEVELS: [&str; 3] = ["low", "medium", "high"];

/// A request asked of the provider configured with `config`.
pub(crate) struct Asked<'p> {
    pub(crate) config: &'p OllamaConfig,
    pub(crate) provider: &'p str,
}

impl Asked<'_> {
    /// The body of the chat answering `request`, whole or `stream`ed.
    ///
    /// # Errors
    ///
    /// A response schema the provider cannot send, one asked beside tools,
    /// and an image that does not read: each before anything is sent.
    pub(crate) async fn body(
        &self,
        request: &AiContext,
        stream: bool,
    ) -> Result<Map<String, Value>, Failure> {
        schema_for_generate(request, true, false, self.provider).map_err(Failure::from)?;
        let messages = self.messages(request).await?;
        let options = self.options(request);
        let think = self.think(request);
        let keep_alive = self.config.keep_alive.as_ref();
        let format = request.response_schema.clone().map(Value::Object);
        let mut body = Map::new();
        body.insert("model".into(), self.config.model.clone().into());
        if request.tools.is_empty() {
            // The SDK's `ChatRequest`, in its members' order.
            body.insert("stream".into(), stream.into());
            if !options.is_empty() {
                body.insert("options".into(), Value::Object(options));
            }
            if let Some(format) = format {
                body.insert("format".into(), format);
            }
            if let Some(keep_alive) = keep_alive {
                body.insert("keep_alive".into(), as_a_float(keep_alive));
            }
            body.insert("messages".into(), messages.into());
            body.insert("tools".into(), Value::Array(Vec::new()));
            if let Some(think) = think {
                body.insert("think".into(), think);
            }
            return Ok(body);
        }
        // RoomKit's own body (`sdk_patch.chat`), in the order the provider
        // sets the members.
        body.insert("messages".into(), messages.into());
        if !options.is_empty() {
            body.insert("options".into(), Value::Object(options));
        }
        if let Some(think) = think {
            body.insert("think".into(), think);
        }
        if let Some(keep_alive) = keep_alive {
            body.insert("keep_alive".into(), keep_alive_value(keep_alive));
        }
        if let Some(format) = format {
            body.insert("format".into(), format);
        }
        let declared = chat_tool_declarations(&request.tools);
        body.insert("tools".into(), declared.into());
        body.insert("stream".into(), stream.into());
        Ok(body)
    }

    /// The options of the request (Python's `_build_options`).
    fn options(&self, request: &AiContext) -> Map<String, Value> {
        let config = self.config;
        let mut options = Map::new();
        options.insert("temperature".into(), request.temperature.into());
        let bound = request
            .max_tokens
            .filter(|bound| *bound != 0)
            .map(i64::from);
        if let Some(bound) = bound.or(config.max_tokens) {
            options.insert("num_predict".into(), bound.into());
        }
        if let Some(num_ctx) = config.num_ctx {
            options.insert("num_ctx".into(), num_ctx.into());
        }
        if let Some(top_p) = config.top_p {
            options.insert("top_p".into(), top_p.into());
        }
        if let Some(top_k) = config.top_k {
            options.insert("top_k".into(), top_k.into());
        }
        if let Some(min_p) = config.min_p {
            options.insert("min_p".into(), min_p.into());
        }
        options
    }

    /// The `think` of the request (Python's `_resolve_think`, RFC §6.7): the
    /// turn states whether the model thinks and how much, the configuration
    /// what it leaves unstated. A level goes only to a model configured with
    /// one, the one sign that it takes levels.
    fn think(&self, request: &AiContext) -> Option<Value> {
        let configured = self.config.think;
        let level = match configured {
            Some(Think::Effort(level)) => Some(level),
            Some(Think::Switch(_)) | None => None,
        };
        let switch = match configured {
            Some(Think::Effort(_)) => Some(true),
            Some(Think::Switch(on)) => Some(on),
            None => None,
        };
        let switch = thinking_switch(request, switch);
        if switch == Some(false) {
            return Some(Value::Bool(false));
        }
        if let Some(level) = level {
            let effort = request.reasoning_effort.as_deref();
            let nearest = nearest_level(effort, &THINK_LEVELS, &EFFORTS);
            return Some(nearest.unwrap_or(level.as_str()).into());
        }
        switch.map(Value::Bool)
    }

    /// The conversation as Ollama reads it, each message as the SDK's
    /// `Message` writes it.
    async fn messages(&self, request: &AiContext) -> Result<Vec<Value>, Failure> {
        let mut written = Vec::new();
        let system = request
            .system_prompt
            .as_deref()
            .filter(|prompt| !prompt.is_empty());
        if let Some(prompt) = system {
            written.push(message(&json!({"role": "system", "content": prompt})));
        }
        for asked in &request.messages {
            for rendered in self.rendered(asked).await? {
                written.push(message(&rendered));
            }
        }
        Ok(written)
    }

    /// One message of the conversation as Python renders it: a tool's
    /// results each its own message, their images on a user message after
    /// them; any other message one message, its texts joined.
    async fn rendered(&self, asked: &AiMessage) -> Result<Vec<Value>, Failure> {
        let role = asked.role.as_str();
        let parts = match &asked.content {
            AiMessageContent::Text(text) => {
                return Ok(vec![json!({"role": role, "content": text})]);
            }
            AiMessageContent::Parts(parts) => parts,
        };
        let results: Vec<&AiToolResultPart> = parts.iter().filter_map(result_of).collect();
        if !results.is_empty() {
            return self.results(&results).await;
        }
        let (mut text, mut thinking, mut images, mut calls) =
            (String::new(), String::new(), Vec::new(), Vec::new());
        for part in parts {
            match part {
                AiContentPart::Text(part) => text.push_str(&part.text),
                AiContentPart::Image(image) => {
                    images.push(Value::String(images::payload(image, self.provider).await?));
                }
                AiContentPart::Thinking(part) => thinking.push_str(&part.thinking),
                AiContentPart::ToolCall(call) => calls.push(json!({
                    "function": {"name": call.name, "arguments": call.arguments},
                })),
                AiContentPart::ToolResult(_) => {}
            }
        }
        let mut written = Map::new();
        written.insert("role".into(), role.into());
        written.insert("content".into(), text.into());
        if !thinking.is_empty() {
            written.insert("thinking".into(), thinking.into());
        }
        if !calls.is_empty() {
            written.insert("tool_calls".into(), calls.into());
        }
        if !images.is_empty() {
            written.insert("images".into(), images.into());
        }
        Ok(vec![Value::Object(written)])
    }

    /// A tool's results, each its own message, their images on one user
    /// message after them: Ollama reads an image on a user message only.
    async fn results(&self, results: &[&AiToolResultPart]) -> Result<Vec<Value>, Failure> {
        let mut written = Vec::new();
        let mut images = Vec::new();
        for result in results {
            let (text, carried) = result.split_for_message();
            written.push(json!({"role": "tool", "content": text, "tool_name": result.name}));
            for image in &carried {
                images.push(Value::String(images::payload(image, self.provider).await?));
            }
        }
        if !images.is_empty() {
            written.push(json!({"role": "user", "content": "", "images": images}));
        }
        Ok(written)
    }
}

fn result_of(part: &AiContentPart) -> Option<&AiToolResultPart> {
    let AiContentPart::ToolResult(result) = part else {
        return None;
    };
    Some(result)
}

/// A count of seconds as the SDK's `ChatRequest` writes it, a number with a
/// fraction; a duration as it is.
fn as_a_float(keep_alive: &KeepAlive) -> Value {
    match keep_alive {
        #[expect(
            clippy::cast_precision_loss,
            reason = "Python's float of the count, as the SDK writes it"
        )]
        KeepAlive::Seconds(seconds) => Value::from(*seconds as f64),
        KeepAlive::Duration(text) => Value::String(text.clone()),
    }
}

/// A message as the SDK's `Message` writes it: its members in their order,
/// an empty one left out.
fn message(rendered: &Value) -> Value {
    let mut written = Map::new();
    for name in [
        "role",
        "content",
        "thinking",
        "images",
        "tool_name",
        "tool_calls",
    ] {
        let Some(member) = rendered.get(name) else {
            continue;
        };
        if is_empty(member) {
            continue;
        }
        written.insert(name.to_owned(), member.clone());
    }
    Value::Object(written)
}

/// Whether Python reads `value` as false, which the SDK leaves out of a
/// message: `None`, an empty text or list.
fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(members) => members.is_empty(),
        Value::Bool(on) => !on,
        Value::Number(number) => number.as_f64() == Some(0.0),
    }
}
