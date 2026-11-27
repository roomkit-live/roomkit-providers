//! The body of a request to `streamGenerateContent` (Python's
//! `build_gen_config` and `format_messages`, as the google-genai SDK writes
//! them): the conversation's contents, its system instruction, the
//! generation config with its thinking config and its response schema, and
//! the tools' declarations.

use roomkit_ai::{AiContext, ProviderError, ProviderErrorOptions, schema_for_generate};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::config::GeminiConfig;
use crate::models::{self, TAKES_LEVELS};
use crate::names;

pub(crate) mod contents;
mod schema;
mod thinking;

/// What a request asks of the model, its configuration and its catalogue
/// entry.
pub(crate) struct Asked<'a> {
    pub(crate) config: &'a GeminiConfig,
    pub(crate) provider: &'a str,
    /// The billing labels the request carries, on Vertex AI.
    pub(crate) labels: Option<&'a [(String, String)]>,
}

impl Asked<'_> {
    /// The body of `request`.
    ///
    /// # Errors
    ///
    /// What is refused before any request: a response schema the contract
    /// refuses, a tool name Gemini does not
    /// take, an image that cannot be sent, a history that ends with the
    /// model.
    pub(crate) fn body(&self, request: &AiContext) -> Result<Map<String, Value>, ProviderError> {
        // Gemini takes a response schema, beside tools too.
        schema_for_generate(request, true, true, self.provider)?;
        let entry = models::entry(&self.config.model);
        let capabilities = models::capabilities(entry);
        let generation = self.generation_config(request, capabilities)?;
        let signed = self.config.thinking_level.is_some()
            || entry.is_none()
            || capabilities.iter().any(|tag| tag == TAKES_LEVELS);
        let contents = contents::contents(&request.messages, signed, self.provider)?;
        self.refuse_a_model_turn_tail(&contents)?;
        let mut body = Map::new();
        body.insert("contents".to_owned(), Value::Array(contents));
        if let Some(prompt) = request.system_prompt.as_deref().filter(|p| !p.is_empty()) {
            let instruction = json!({"parts": [{"text": prompt}], "role": "user"});
            body.insert("systemInstruction".to_owned(), instruction);
        }
        if !request.tools.is_empty() {
            let declarations: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| {
                    let parameters = roomkit_ai::declared_parameters(Some(&tool.parameters));
                    json!({
                        "description": tool.description,
                        "name": tool.name,
                        "parameters": schema::declared(&parameters),
                    })
                })
                .collect();
            body.insert(
                "tools".to_owned(),
                json!([{"functionDeclarations": declarations}]),
            );
        }
        // The SDK writes the tools, then the labels, before the generation
        // config.
        if let Some(labels) = self.labels {
            let labels = labels
                .iter()
                .map(|(key, value)| (key.clone(), value.as_str().into()));
            body.insert("labels".to_owned(), Value::Object(labels.collect()));
        }
        body.insert("generationConfig".to_owned(), generation);
        Ok(body)
    }

    /// The generation config: the turn's temperature and token bound, its
    /// thinking config, its response schema.
    fn generation_config(
        &self,
        request: &AiContext,
        capabilities: &[String],
    ) -> Result<Value, ProviderError> {
        names::check(&request.tools, self.provider)?;
        let mut generation = Map::new();
        generation.insert("temperature".to_owned(), request.temperature.into());
        let bound = request.max_tokens.filter(|bound| *bound != 0);
        let bound = bound.unwrap_or(self.config.max_tokens);
        generation.insert("maxOutputTokens".to_owned(), bound.into());
        let configured = self.config.thinking_level.as_deref();
        if let Some(thinking) = thinking::config(request, configured, capabilities) {
            generation.insert("thinkingConfig".to_owned(), thinking);
        }
        if let Some(schema) = &request.response_schema {
            generation.insert("responseMimeType".to_owned(), "application/json".into());
            generation.insert(
                "responseJsonSchema".to_owned(),
                Value::Object(schema.clone()),
            );
        }
        Ok(Value::Object(generation))
    }

    /// Refuses a history that ends with the model speaking, which Gemini
    /// answers with a 400: the cause is upstream, a turn generated with no
    /// new input to answer.
    fn refuse_a_model_turn_tail(&self, contents: &[Value]) -> Result<(), ProviderError> {
        let model = |content: &&Value| content.get("role").and_then(Value::as_str) == Some("model");
        let tail = contents.iter().rev().take_while(model).count();
        if tail == 0 {
            return Ok(());
        }
        let message = format!(
            "Gemini requires the request to end with a user turn; the history ends with a \
             model turn ({tail} trailing model content(s)). Append the triggering user \
             message, or drop the trailing model turn, before generating."
        );
        let options = ProviderErrorOptions {
            provider: self.provider.to_owned(),
            ..ProviderErrorOptions::default()
        };
        Err(ProviderError::new(message, options))
    }
}

/// `value` as Python's `json.dumps(value, ensure_ascii=False)` writes it,
/// `", "` and `": "` between its members, the line separators a JSON string
/// keeps raw escaped (Python's `json_line`).
pub(crate) fn json_line(value: &Value) -> String {
    let mut written = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut written, PythonFormatter);
    // Writing a JSON value to memory cannot fail.
    let _written = value.serialize(&mut serializer);
    String::from_utf8_lossy(&written)
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .replace('\u{85}', "\\u0085")
}

/// The separators of Python's `json.dumps`.
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
