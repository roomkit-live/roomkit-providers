//! The body of a Chat Completions request: Python's `generate`, its
//! refusals in Python's order and its members in the order Python's client
//! writes them.

use std::sync::atomic::{AtomicBool, Ordering};

use roomkit_ai::{
    AiContext, AiTool, ModelInfo, ProviderError, ProviderErrorOptions, declared_parameters,
    schema_for_generate,
};
use serde_json::{Map, Value, json};

use crate::config::OpenAiConfig;
use crate::messages::chat_messages;
use crate::models::{
    CHAT_TOOLS_REFUSED, RESPONSES_ONLY, TOOLS_REASONING_EFFORT, TOOLS_REASONING_NONE,
    reasoning_floor, tagged,
};
use crate::names;

/// What a request is written for: the configuration, and what the provider
/// knows of the endpoint.
pub(crate) struct Asked<'p> {
    pub(crate) config: &'p OpenAiConfig,
    /// The provider's name, which a failure carries.
    pub(crate) provider: &'p str,
    /// The catalogue's entry of the model on `OpenAI`'s own endpoint;
    /// `None` behind a base URL, where the server decides (RFC §6.7).
    pub(crate) entry: Option<&'static ModelInfo>,
    /// Whether the endpoint is `OpenAI`'s own.
    pub(crate) own_endpoint: bool,
    /// Whether the provider already warned of a reasoning setting left out.
    pub(crate) left_out_warned: &'p AtomicBool,
}

impl Asked<'_> {
    /// The body of the request, asking for a stream when `streamed`; a
    /// member of the configuration's extra body replaces the provider's own
    /// of the same name, as Python's client merges it last.
    ///
    /// # Errors
    ///
    /// What is refused before any request, in Python's order: a model the
    /// endpoint does not serve, an image that cannot be sent, a response
    /// schema the server does not take, a tool name the vendor refuses.
    pub(crate) fn body(
        &self,
        request: &AiContext,
        streamed: bool,
    ) -> Result<Map<String, Value>, ProviderError> {
        self.check_model_serves(request)?;
        let field = self.config.thinking_field.as_deref();
        let prompt = request.system_prompt.as_deref();
        let messages = chat_messages(&request.messages, prompt, field, self.provider)?;
        let effort = self.reasoning_effort(request);
        let schema = schema_for_generate(
            request,
            self.supports_response_schema(),
            self.supports_response_schema_with_tools(),
            self.provider,
        )?;
        let tools = self.declared(&request.tools)?;

        let mut body = Map::new();
        body.insert("model".to_owned(), self.config.model.as_str().into());
        if streamed {
            body.insert("stream".to_owned(), true.into());
        }
        body.insert("messages".to_owned(), messages.into());
        // A bound of zero is read as none, as Python reads it.
        let bound = request.max_tokens.filter(|bound| *bound != 0);
        if let Some(bound) = bound.or(self.config.max_tokens) {
            let modern = self.config.takes_max_completion_tokens();
            let name = if modern {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            body.insert(name.to_owned(), bound.into());
        }
        if let Some(effort) = effort {
            body.insert("reasoning_effort".to_owned(), effort.into());
        }
        if let Some(schema) = schema {
            body.insert("response_format".to_owned(), json_schema_format(schema));
        }
        // A streamed answer reports its usage unless the configuration
        // says the server refuses the option.
        if streamed && self.config.include_stream_usage {
            let options = Map::from_iter([("include_usage".to_owned(), true.into())]);
            body.insert("stream_options".to_owned(), options.into());
        }
        if self.config.takes_temperature() {
            body.insert("temperature".to_owned(), request.temperature.into());
        }
        if let Some(tools) = tools {
            body.insert("tools".to_owned(), tools.into());
        }
        for (name, value) in self.config.extra_body.iter().flatten() {
            body.insert(name.clone(), value.clone());
        }
        Ok(body)
    }

    /// The config's answer when it gives one, else yes: `OpenAI`'s endpoint
    /// honours a `json_schema` response format.
    pub(crate) fn supports_response_schema(&self) -> bool {
        self.config.supports_response_schema.unwrap_or(true)
    }

    /// The config's answer when it gives one, else on `OpenAI`'s own
    /// endpoint alone: a grammar-constrained server cannot call a tool
    /// under the constraint.
    pub(crate) fn supports_response_schema_with_tools(&self) -> bool {
        let configured = self.config.supports_response_schema_with_tools;
        configured.unwrap_or_else(|| self.supports_response_schema() && self.own_endpoint)
    }

    /// Refuses, before the request, a turn `OpenAI`'s endpoint refuses its
    /// model, as the catalogue states it (Python's `_check_model_serves`).
    fn check_model_serves(&self, request: &AiContext) -> Result<(), ProviderError> {
        let model = &self.config.model;
        let refused = if tagged(self.entry, RESPONSES_ONLY) {
            format!("{model} is served by OpenAI's Responses API only, not Chat Completions")
        } else if !request.tools.is_empty() && tagged(self.entry, CHAT_TOOLS_REFUSED) {
            format!("{model} takes no function tools on Chat Completions")
        } else {
            return Ok(());
        };
        let options = ProviderErrorOptions {
            provider: self.provider.to_owned(),
            context_overflow: Some(false),
            before_request: true,
            ..ProviderErrorOptions::default()
        };
        Err(ProviderError::new(refused, options))
    }

    /// The `reasoning_effort` the request carries (RFC §6.7, Python's
    /// `_apply_sampling_kwargs`): the request's effort over the configured
    /// one; the model's least effort where the request states off and the
    /// catalogue knows it; then, on a turn with tools, what the endpoint
    /// accepts there.
    fn reasoning_effort(&self, request: &AiContext) -> Option<String> {
        let stated = request.reasoning_effort.as_deref();
        let effort = stated.or(self.config.reasoning_effort.as_deref());
        // The budget, then the switch, then an effort of `none` state off.
        let switch = match (request.thinking_budget, request.enable_thinking) {
            (Some(budget), _) => Some(budget > 0),
            (None, Some(enabled)) => Some(enabled),
            (None, None) => (stated == Some("none")).then_some(false),
        };
        let off = switch == Some(false) || effort == Some("none");
        let floor = reasoning_floor(self.entry).filter(|_| off);
        let effort = floor.or(effort);
        if request.tools.is_empty() {
            return effort.map(str::to_owned);
        }
        if tagged(self.entry, TOOLS_REASONING_NONE) {
            return Some("none".to_owned());
        }
        if tagged(self.entry, TOOLS_REASONING_EFFORT) {
            return effort.map(str::to_owned);
        }
        // No catalogue speaks for the model: the config says what the
        // server takes beside tools, and an unsaid one is left out.
        let says = self.config.supports_reasoning_effort_with_tools;
        if let (None, Some(effort)) = (says, effort) {
            self.warn_left_out(effort);
        }
        effort.filter(|_| says == Some(true)).map(str::to_owned)
    }

    /// Warns, once per provider, that a turn with tools left out `effort`.
    fn warn_left_out(&self, effort: &str) {
        if self.left_out_warned.swap(true, Ordering::Relaxed) {
            return;
        }
        tracing::warn!(
            target: "roomkit::providers::openai",
            provider = self.provider,
            effort,
            model = %self.config.model,
            "a reasoning setting is left out of a turn with tools, since nothing says the \
             server takes it there; set supports_reasoning_effort_with_tools on the \
             configuration (RFC 6.7)"
        );
    }

    /// The request's tools as the endpoint declares them, their names
    /// checked on `OpenAI`'s own endpoint; `None` for a request without.
    fn declared(&self, tools: &[AiTool]) -> Result<Option<Vec<Value>>, ProviderError> {
        if tools.is_empty() {
            return Ok(None);
        }
        if self.own_endpoint {
            names::check(tools, self.provider)?;
        }
        let declared = tools.iter().map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": declared_parameters(Some(&tool.parameters)),
                },
            })
        });
        Ok(Some(declared.collect()))
    }
}

/// The `response_format` that constrains an answer to `schema`, strictly.
fn json_schema_format(schema: &Map<String, Value>) -> Value {
    json!({
        "type": "json_schema",
        "json_schema": {"name": "response", "schema": schema, "strict": true},
    })
}
