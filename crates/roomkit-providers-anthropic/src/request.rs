//! The body of a Messages request: Python's `build_kwargs`, its refusals in
//! Python's order and its members in the order Python's client writes them.

mod blocks;
mod cache;

use roomkit_providers_llm::{
    AiRequest, AiTool, ModelInfo, ProviderError, declared_parameters, schema_for_generate,
};
use serde_json::{Map, Value, json};

use crate::config::AnthropicConfig;
use crate::names;
use crate::reasoning;

/// What a request is written for: the configuration, and what the provider
/// knows of the model and of the endpoint.
pub(crate) struct Asked<'p> {
    pub(crate) config: &'p AnthropicConfig,
    /// The provider's name, which a failure carries.
    pub(crate) provider: &'p str,
    /// The catalogue's entry of the model, whatever the endpoint: it says
    /// how the model's reasoning settings are spelled.
    pub(crate) entry: Option<&'static ModelInfo>,
    /// Whether the endpoint is Anthropic's own, whose rule of tool names
    /// applies.
    pub(crate) own_endpoint: bool,
}

impl Asked<'_> {
    /// The body of the request, which always asks for a stream: `generate`
    /// reads the stream the other calls give.
    ///
    /// # Errors
    ///
    /// What is refused before any request, in Python's order: a response
    /// schema outside the portable subset, an image that cannot be sent, a
    /// tool name Anthropic refuses.
    pub(crate) fn body(&self, request: &AiRequest) -> Result<Map<String, Value>, ProviderError> {
        let schema = schema_for_generate(request, true, true, self.provider)?;
        let mut messages = blocks::messages(&request.messages, self.provider)?;
        let sampling = reasoning::sampling(self.config, request, self.entry);
        let mut tools = self.declared(&request.tools)?;
        let prompt = request.system_prompt.as_deref();
        let mut system = prompt.filter(|prompt| !prompt.is_empty()).map(Value::from);
        if self.config.enable_prompt_caching {
            cache::mark(tools.as_deref_mut(), system.as_mut(), &mut messages);
        }
        let effort = reasoning::effort(request, self.entry);

        let mut body = Map::new();
        // A bound of zero is read as none, as Python reads it.
        let bound = request.max_tokens.filter(|bound| *bound != 0);
        let bound = bound.unwrap_or(self.config.max_tokens);
        body.insert("max_tokens".to_owned(), bound.into());
        body.insert("messages".to_owned(), messages.into());
        body.insert("model".to_owned(), self.config.model.as_str().into());
        if let Some(output) = output_config(effort, schema) {
            body.insert("output_config".to_owned(), output);
        }
        if let Some(system) = system {
            body.insert("system".to_owned(), system);
        }
        if let Some(tools) = tools {
            body.insert("tools".to_owned(), tools.into());
        }
        if let Some(thinking) = sampling.thinking {
            body.insert("thinking".to_owned(), thinking);
        }
        body.insert("stream".to_owned(), true.into());
        if sampling.temperature {
            body.insert("temperature".to_owned(), request.temperature.into());
        }
        Ok(body)
    }

    /// The request's tools as Anthropic declares them, their names checked
    /// on Anthropic's own endpoint; `None` for a request without. A tool
    /// held unseen is declared so, unless every tool is: Anthropic refuses
    /// such a request, and then none is held.
    fn declared(&self, tools: &[AiTool]) -> Result<Option<Vec<Value>>, ProviderError> {
        if tools.is_empty() {
            return Ok(None);
        }
        if self.own_endpoint {
            names::check(tools, self.provider)?;
        }
        let defers = !tools.iter().all(|tool| tool.defer_loading);
        let declared = tools.iter().map(|tool| {
            let mut declared = json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": declared_parameters(Some(&tool.parameters)),
            });
            if defers && tool.defer_loading {
                cache::put(&mut declared, "defer_loading", true.into());
            }
            declared
        });
        Ok(Some(declared.collect()))
    }
}

/// The turn's `output_config`: its effort and the schema of its answer;
/// `None` for a turn that states neither.
fn output_config(effort: Option<&str>, schema: Option<&Map<String, Value>>) -> Option<Value> {
    let mut output = Map::new();
    if let Some(effort) = effort {
        output.insert("effort".to_owned(), effort.into());
    }
    if let Some(schema) = schema {
        let format = json!({"type": "json_schema", "schema": schema});
        output.insert("format".to_owned(), format);
    }
    (!output.is_empty()).then(|| output.into())
}
