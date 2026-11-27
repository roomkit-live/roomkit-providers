//! A request as PolarGrid's chat completions read it: Python's
//! `PolarGridAIProvider._build_request`, its body written as polargrid-sdk
//! writes it (`_build_chat_completion_body`), with the values its builder
//! would replace sent as given (`sdk_patch`).
//!
//! The conversation is the one every provider of a chat completions wire
//! renders (RoomKit's `chat_messages`): text sent flat, blocks only for an
//! image, each tool message naming its tool. The SDK reads each message
//! into its `Message` and writes it back, its members in that model's
//! order. Before anything is sent, the SDK refuses a temperature past 0 to
//! 2 and a nucleus probability past 0 to 1.

use roomkit_ai::{
    AiContext, ChatDialect, ChatDialectOptions, ProviderError, ProviderErrorOptions, chat_messages,
    chat_tool_declarations, schema_for_generate,
};
use roomkit_providers_client::reasoning::thinking_switch;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::catalogue;
use crate::config::PolarGridConfig;
use crate::failure::{self, Failure};

/// The most output tokens a request may ask for: the SDK refuses more.
pub(crate) const MAX_TOKENS: u32 = 4096;

/// A request asked of the provider configured with `config`.
pub(crate) struct Asked<'p> {
    pub(crate) config: &'p PolarGridConfig,
    pub(crate) provider: &'p str,
}

impl Asked<'_> {
    /// The body of the chat completion answering `request`, whole or
    /// `stream`ed, its members in the SDK's order.
    ///
    /// # Errors
    ///
    /// A response schema the provider cannot send, one asked beside tools,
    /// a message that cannot be rendered, and a value the SDK refuses: each
    /// before anything is sent.
    pub(crate) fn body(
        &self,
        request: &AiContext,
        stream: bool,
    ) -> Result<Map<String, Value>, Failure> {
        schema_for_generate(request, true, false, self.provider)?;
        let rendered = self.messages(request)?;
        let declared = chat_tool_declarations(&request.tools);
        let declared = Some(declared).filter(|declared| !declared.is_empty());
        let max_tokens = self.max_tokens(request, &rendered, declared.as_deref());
        checked(request.temperature, self.config.top_p)?;
        let mut body = Map::new();
        body.insert("model".into(), self.config.model.clone().into());
        let messages = rendered.iter().map(message).collect::<Vec<_>>();
        body.insert("messages".into(), messages.into());
        body.insert("max_tokens".into(), max_tokens.into());
        body.insert("temperature".into(), request.temperature.into());
        body.insert("top_p".into(), self.config.top_p.into());
        body.insert("stream".into(), stream.into());
        if let Some(declared) = declared {
            body.insert(
                "tools".into(),
                declared.iter().map(tool).collect::<Vec<_>>().into(),
            );
        }
        if let Some(schema) = &request.response_schema {
            let format = json!({
                "type": "json_schema",
                "json_schema": {"name": "response", "schema": schema, "strict": true},
            });
            body.insert("response_format".into(), format);
        }
        if let Some(thinking) = thinking_switch(request, self.config.thinking) {
            body.insert("enable_thinking".into(), thinking.into());
        }
        if stream {
            body.insert("stream_options".into(), json!({"include_usage": true}));
        }
        Ok(body)
    }

    /// The conversation as RoomKit renders it for PolarGrid.
    fn messages(&self, request: &AiContext) -> Result<Vec<Value>, ProviderError> {
        let options = ChatDialectOptions {
            names_tool_results: true,
            flattens_text: true,
            ..ChatDialectOptions::default()
        };
        let dialect = ChatDialect::new(options).map_err(|error| {
            let options = ProviderErrorOptions {
                provider: self.provider.to_owned(),
                ..ProviderErrorOptions::default()
            };
            ProviderError::new(error.to_string(), options)
        })?;
        let system = request.system_prompt.as_deref();
        chat_messages(&request.messages, system, &dialect, self.provider)
    }

    /// The output bound of the request (Python's `_max_tokens`): the turn's
    /// or the configured one, at most the SDK's, else what the model's
    /// window leaves after the prompt (`_room_left`).
    fn max_tokens(&self, request: &AiContext, messages: &[Value], tools: Option<&[Value]>) -> u32 {
        let asked = request.max_tokens.filter(|bound| *bound != 0);
        let Some(asked) = asked.or(self.config.max_tokens) else {
            return room_left(&self.config.model, messages, tools);
        };
        if asked > MAX_TOKENS {
            tracing::warn!(
                target: "roomkit::providers::polargrid",
                asked,
                sent = MAX_TOKENS,
                "polargrid-sdk caps max_tokens at {MAX_TOKENS}"
            );
        }
        asked.min(MAX_TOKENS)
    }
}

/// What the window of `model` leaves an answer after the prompt, at most
/// the SDK's bound: the prompt over-counted at a token every three
/// characters of its JSON, as Python writes it (`json.dumps`, characters
/// past ASCII as they are). A model the catalogue gives no window gets the
/// bound.
fn room_left(model: &str, messages: &[Value], tools: Option<&[Value]>) -> u32 {
    let window = catalogue::entry(model).and_then(|entry| entry.context_window);
    let Some(window) = window else {
        return MAX_TOKENS;
    };
    let sent = json!([messages, tools]);
    let prompt = python_json(&sent).chars().count() / 3;
    let left =
        i64::try_from(window).unwrap_or(i64::MAX) - i64::try_from(prompt).unwrap_or(i64::MAX);
    let left = left.clamp(1, i64::from(MAX_TOKENS));
    u32::try_from(left).unwrap_or(MAX_TOKENS)
}

/// `value` as Python's `json.dumps(value, ensure_ascii=False)` writes it:
/// `", "` and `": "` between members, every character as itself.
fn python_json(value: &Value) -> String {
    let mut written = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut written, PythonFormatter);
    if value.serialize(&mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(written).unwrap_or_default()
}

/// Python's separators.
struct PythonFormatter;

impl serde_json::ser::Formatter for PythonFormatter {
    fn begin_array_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> std::io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(b", ")
        }
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
    ) -> std::io::Result<()> {
        writer.write_all(b": ")
    }
}

/// The SDK's checks of a request's sampling (its `ChatCompletionRequest`).
fn checked(temperature: f64, top_p: f64) -> Result<(), Failure> {
    if !(0.0..=2.0).contains(&temperature) {
        return Err(failure::refused_by_the_sdk(
            "temperature must be between 0.0 and 2.0",
        ));
    }
    if !(0.0..=1.0).contains(&top_p) {
        return Err(failure::refused_by_the_sdk(
            "top_p must be between 0.0 and 1.0",
        ));
    }
    Ok(())
}

/// The members of `value` named in `order`, in that order, a `null` one
/// left out: what a model of the SDK writes back with
/// `model_dump(exclude_none=True)`.
fn ordered(value: &Value, order: &[&str]) -> Map<String, Value> {
    let mut written = Map::new();
    for name in order {
        if let Some(member) = value.get(*name).filter(|member| !member.is_null()) {
            written.insert((*name).to_owned(), member.clone());
        }
    }
    written
}

/// A message as the SDK's `Message` writes it back.
fn message(rendered: &Value) -> Value {
    Value::Object(ordered(
        rendered,
        &["role", "content", "name", "tool_calls", "tool_call_id"],
    ))
}

/// A tool's declaration as the SDK's `ChatTool` writes it back.
fn tool(declared: &Value) -> Value {
    let mut written = Map::new();
    let kind = declared
        .get("type")
        .cloned()
        .unwrap_or_else(|| "function".into());
    written.insert("type".into(), kind);
    if let Some(function) = declared.get("function") {
        let function = ordered(function, &["name", "description", "parameters"]);
        written.insert("function".into(), Value::Object(function));
    }
    Value::Object(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Python: json.dumps([{"a": "é"}, None], ensure_ascii=False)
    #[test]
    fn the_json_is_written_as_python_writes_it() {
        assert_eq!(
            python_json(&json!([{"a": "é", "b": [1, 2]}, null])),
            r#"[{"a": "é", "b": [1, 2]}, null]"#
        );
    }

    #[test]
    fn the_room_left_is_the_window_less_a_third_of_the_prompt() {
        let short = [json!({"role": "user", "content": "Hello"})];
        assert_eq!(room_left("qwen-3.8-27b", &short, None), MAX_TOKENS);
        assert_eq!(room_left("no-such-model", &short, None), MAX_TOKENS);
        let long = [json!({"role": "user", "content": "x".repeat(30_000)})];
        assert_eq!(room_left("qwen-3.6-35b-a3b", &long, None), 1);
        let filling = [json!({"role": "user", "content": "x".repeat(15_000)})];
        let prompt = python_json(&json!([filling, null])).chars().count() / 3;
        let expected = u32::try_from(8192 - prompt).unwrap();
        assert_eq!(room_left("qwen-3.6-35b-a3b", &filling, None), expected);
    }
}
