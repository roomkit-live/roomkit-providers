//! A request as Mistral's chat completions read it: Python's
//! `MistralAIProvider._build_kwargs`, its body written as the mistralai SDK
//! writes it.
//!
//! The conversation is the one every provider of a chat completions wire
//! renders (RoomKit's `chat_messages`, each tool message naming its tool).
//! The SDK reads each message, and each part of one, into its own models
//! and writes them back: their members in the order its models declare
//! them, a member they do not declare left out, and the defaults it adds
//! (an assistant message's `prefix`, a call's `type` and `index`).

use roomkit_ai::{
    AiContext, ChatDialect, ChatDialectOptions, ProviderError, ProviderErrorOptions, chat_messages,
    chat_tool_declarations, schema_for_generate,
};
use roomkit_providers_client::reasoning::thinking_switch;
use serde_json::{Map, Value, json};

use crate::config::MistralConfig;
use crate::names;

/// A request asked of the provider configured with `config`: whether it
/// asks Mistral's own endpoint, whose tool names it knows.
pub(crate) struct Asked<'p> {
    pub(crate) config: &'p MistralConfig,
    pub(crate) provider: &'p str,
    pub(crate) own_endpoint: bool,
}

impl Asked<'_> {
    /// The body of the streamed chat completion answering `request`, its
    /// members in the SDK's order.
    ///
    /// # Errors
    ///
    /// A response schema the provider cannot send, one asked beside tools,
    /// a message that cannot be rendered, and a tool name Mistral's own
    /// endpoint refuses: each before anything is sent.
    pub(crate) fn body(&self, request: &AiContext) -> Result<Map<String, Value>, ProviderError> {
        schema_for_generate(request, true, false, self.provider)?;
        let mut body = Map::new();
        body.insert("model".into(), self.config.model.clone().into());
        body.insert("messages".into(), self.messages(request)?.into());
        body.insert("temperature".into(), request.temperature.into());
        let max_tokens = request.max_tokens.filter(|bound| *bound != 0);
        let max_tokens = max_tokens.unwrap_or(self.config.max_tokens);
        body.insert("max_tokens".into(), max_tokens.into());
        body.insert("stream".into(), true.into());
        if let Some(schema) = &request.response_schema {
            body.insert("response_format".into(), json_schema_format(schema));
        }
        if !request.tools.is_empty() {
            if self.own_endpoint {
                names::check(&request.tools, self.provider)?;
            }
            let declared = chat_tool_declarations(&request.tools);
            let declared = declared.iter().map(tool).collect::<Vec<_>>();
            body.insert("tools".into(), declared.into());
        }
        if let Some(effort) = reasoning_effort(request, self.config) {
            body.insert("reasoning_effort".into(), effort.into());
        }
        Ok(body)
    }

    /// The conversation, each message as the SDK writes it back.
    fn messages(&self, request: &AiContext) -> Result<Vec<Value>, ProviderError> {
        let options = ChatDialectOptions {
            names_tool_results: true,
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
        let rendered = chat_messages(&request.messages, system, &dialect, self.provider)?;
        Ok(rendered.iter().map(message).collect())
    }
}

/// The reasoning effort a turn sends (Python's `_resolve_reasoning_effort`,
/// RFC §6.7): the turn's effort over the configured one; a turn that states
/// reasoning off sends `none`, one that states it on the effort or `high`;
/// `None` leaves the model's default, a Magistral model reasoning always.
fn reasoning_effort<'a>(request: &'a AiContext, config: &'a MistralConfig) -> Option<&'a str> {
    let effort = request.reasoning_effort.as_deref();
    let effort = effort.or(config.reasoning_effort.as_deref());
    match thinking_switch(request, None) {
        None => effort,
        Some(false) => Some("none"),
        Some(true) => Some(effort.unwrap_or("high")),
    }
}

/// The `response_format` that constrains an answer to `schema`, strictly
/// (Python's `json_schema_format`).
fn json_schema_format(schema: &Map<String, Value>) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {"name": "response", "schema": schema, "strict": true},
    })
}

/// The members of `value` named in `order`, in that order: what a model of
/// the SDK keeps of a member it declares, `null` included.
fn ordered(value: &Value, order: &[&str]) -> Map<String, Value> {
    let mut written = Map::new();
    for name in order {
        if let Some(member) = value.get(*name) {
            written.insert((*name).to_owned(), member.clone());
        }
    }
    written
}

/// A message as the SDK writes it back: by its role's model.
fn message(rendered: &Value) -> Value {
    let role = rendered.get("role").and_then(Value::as_str);
    let mut written = match role {
        Some("assistant") => ordered(rendered, &["role", "content", "tool_calls", "prefix"]),
        Some("tool") => ordered(rendered, &["content", "role", "tool_call_id", "name"]),
        _ => ordered(rendered, &["content", "role"]),
    };
    if let Some(content) = written.get_mut("content") {
        *content = written_content(content);
    }
    if role == Some("assistant") {
        if let Some(Value::Array(calls)) = written.get_mut("tool_calls") {
            for call in calls.iter_mut() {
                *call = tool_call(call);
            }
        }
        written.insert("prefix".into(), false.into());
    }
    Value::Object(written)
}

/// A message's content: a text as it is, each part of a list as the SDK's
/// chunk of its type writes it.
fn written_content(content: &Value) -> Value {
    let Value::Array(parts) = content else {
        return content.clone();
    };
    let parts = parts.iter().map(|part| {
        let kind = part.get("type").and_then(Value::as_str);
        if kind != Some("image_url") {
            return Value::Object(ordered(part, &["text", "type"]));
        }
        let mut written = ordered(part, &["image_url", "type"]);
        if let Some(image @ Value::Object(_)) = written.get_mut("image_url") {
            *image = Value::Object(ordered(image, &["url", "detail"]));
        }
        Value::Object(written)
    });
    Value::Array(parts.collect())
}

/// A call of an assistant message as the SDK writes it back: its function,
/// its id, its type and its index, which default to `function` and 0.
fn tool_call(call: &Value) -> Value {
    let mut written = Map::new();
    if let Some(function) = call.get("function") {
        let function = ordered(function, &["name", "arguments"]);
        written.insert("function".into(), Value::Object(function));
    }
    if let Some(id) = call.get("id") {
        written.insert("id".into(), id.clone());
    }
    let kind = call
        .get("type")
        .cloned()
        .unwrap_or_else(|| "function".into());
    written.insert("type".into(), kind);
    let index = call.get("index").cloned().unwrap_or_else(|| 0.into());
    written.insert("index".into(), index);
    Value::Object(written)
}

/// A tool's declaration as the SDK writes it back: its function, then its
/// type.
fn tool(declared: &Value) -> Value {
    let mut written = Map::new();
    if let Some(function) = declared.get("function") {
        let function = ordered(function, &["name", "parameters", "description"]);
        written.insert("function".into(), Value::Object(function));
    }
    let kind = declared
        .get("type")
        .cloned()
        .unwrap_or_else(|| "function".into());
    written.insert("type".into(), kind);
    Value::Object(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(reasoning_effort: Option<&str>, thinking_budget: Option<i64>) -> AiContext {
        AiContext {
            reasoning_effort: reasoning_effort.map(str::to_owned),
            thinking_budget,
            ..AiContext::default()
        }
    }

    // Python: providers/mistral/ai.py _resolve_reasoning_effort
    #[test]
    fn the_turn_s_effort_outranks_the_configured_one_and_a_switch_decides() {
        let config = MistralConfig {
            reasoning_effort: Some("high".into()),
            ..MistralConfig::new("k")
        };
        let effort = |request: &AiContext| reasoning_effort(request, &config).map(str::to_owned);
        assert_eq!(effort(&asked(None, None)).as_deref(), Some("high"));
        assert_eq!(effort(&asked(Some("low"), None)).as_deref(), Some("low"));
        assert_eq!(effort(&asked(None, Some(0))).as_deref(), Some("none"));
        assert_eq!(effort(&asked(Some("none"), None)).as_deref(), Some("none"));
        let unset = MistralConfig::new("k");
        let effort = |request: &AiContext| reasoning_effort(request, &unset).map(str::to_owned);
        assert_eq!(effort(&asked(None, None)), None);
        assert_eq!(effort(&asked(None, Some(2048))).as_deref(), Some("high"));
    }
}
