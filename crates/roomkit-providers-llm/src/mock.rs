//! A provider answering from a list, round-robin, for tests and examples:
//! Python's `roomkit.providers.ai.MockAIProvider`.

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;

use crate::Metadata;
use crate::catalog::ModelInfo;
use crate::error::ProviderError;
use crate::provider::{AiProvider, ProviderStream, TextStream};
use crate::python::dumps;
use crate::request::AiRequest;
use crate::response::{
    AiResponse, StreamDone, StreamEvent, StreamToolCall, StreamToolCallDelta, response_events,
};
use crate::round::is_malformed_call;
use crate::schema::{check_schema_answer, checked_stream, schema_for_generate};
use crate::stream::{Yielder, items, next};

/// What [`MockAiProvider::new`] takes: Python's `responses=None, *,
/// vision=False, ai_responses=None, streaming=False,
/// tool_call_delta_chunks=0, response_schema=False,
/// response_schema_with_tools=False`.
#[derive(Clone, Debug, Default)]
#[expect(clippy::struct_excessive_bools, reason = "Python's keywords")]
pub struct MockAiProviderOptions {
    /// The texts it answers, round-robin; empty answers `Hello from AI`.
    pub responses: Vec<String>,
    /// Whether its model takes images.
    pub vision: bool,
    /// The whole responses it answers, round-robin, in place of
    /// `responses`.
    pub ai_responses: Option<Vec<AiResponse>>,
    /// Whether it says it streams (`supports_streaming`,
    /// `supports_structured_streaming`); either way its structured stream
    /// sends each response as its events.
    pub streaming: bool,
    /// Split each call's JSON arguments into this many fragments before
    /// its complete call. Zero (the default) or negative emits none; a mock
    /// that does not stream emits none either.
    pub tool_call_delta_chunks: i64,
    /// Whether it honours [`AiRequest::response_schema`] as a real provider
    /// must (RFC §6.7), streaming included: the answer must be a JSON
    /// document satisfying the schema, a response ending `refusal` is a
    /// refusal and one that ran out of room is truncated, each failing with
    /// its [`ResponseSchemaError`](crate::ResponseSchemaError);
    /// `supports_response_schema`.
    pub response_schema: bool,
    /// Whether it takes a response schema beside tools
    /// (`supports_response_schema_with_tools`).
    pub response_schema_with_tools: bool,
}

/// A provider answering each generation with the next of its responses,
/// round-robin, and keeping each request it was asked: Python's
/// `MockAIProvider`. Its model is `mock`. Cloning it shares its responses
/// and the requests it kept, so a test keeps a clone to read them once
/// something else holds the provider.
///
/// ```
/// use roomkit_providers_llm::{AiProvider, MockAiProvider, MockAiProviderOptions};
///
/// let model = MockAiProvider::new(MockAiProviderOptions {
///     responses: vec!["Order 42 shipped on Monday.".into()],
///     ..MockAiProviderOptions::default()
/// });
/// assert_eq!(model.model_name(), "mock");
/// assert_eq!(model.context_window(), Some(8192));
/// assert!(model.calls().is_empty());
/// ```
#[derive(Clone, Debug)]
pub struct MockAiProvider {
    responses: Arc<Vec<AiResponse>>,
    vision: bool,
    streaming: bool,
    tool_call_delta_chunks: i64,
    /// `response_schema` and `response_schema_with_tools`.
    schema: (bool, bool),
    shared: Arc<Mutex<Asked>>,
}

/// What the provider was asked: each request, and the next response.
#[derive(Debug, Default)]
struct Asked {
    calls: Vec<AiRequest>,
    next: usize,
}

impl MockAiProvider {
    /// The provider answering as `options` says.
    pub fn new(options: MockAiProviderOptions) -> Self {
        let MockAiProviderOptions {
            responses,
            vision,
            ai_responses,
            streaming,
            tool_call_delta_chunks,
            response_schema,
            response_schema_with_tools,
        } = options;
        let responses = match ai_responses.filter(|whole| !whole.is_empty()) {
            Some(whole) => whole,
            None if responses.is_empty() => vec![text_response("Hello from AI")],
            None => responses
                .iter()
                .map(String::as_str)
                .map(text_response)
                .collect(),
        };
        Self {
            responses: Arc::new(responses),
            vision,
            streaming,
            tool_call_delta_chunks,
            schema: (response_schema, response_schema_with_tools),
            shared: Arc::default(),
        }
    }

    /// The requests it was asked, in order: Python's `calls`.
    pub fn calls(&self) -> Vec<AiRequest> {
        self.lock().calls.clone()
    }

    /// Keeps `request` and takes the next response, round-robin.
    fn answer(&self, request: &AiRequest) -> AiResponse {
        let mut asked = self.lock();
        asked.calls.push(request.clone());
        let at = asked.next % self.responses.len().max(1);
        asked.next = asked.next.wrapping_add(1);
        self.responses
            .get(at)
            .cloned()
            .unwrap_or_else(|| text_response("Hello from AI"))
    }

    /// Refuses, before `request` is kept, a schema it cannot take.
    fn refuse_unsupported(&self, request: &AiRequest) -> Result<(), ProviderError> {
        let (supported, with_tools) = self.schema;
        schema_for_generate(request, supported, with_tools, "mock").map(|_| ())
    }

    /// The next response to `request`, a constrained answer checked as a
    /// real provider checks it: a response that calls, or that ended on a
    /// call its provider could not hand over, is no answer (RFC §6.4).
    fn checked_answer(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        self.refuse_unsupported(request)?;
        let response = self.answer(request);
        let finish = response.finish_reason.as_deref();
        if let Some(schema) = &request.response_schema
            && response.tool_calls.is_empty()
            && !is_malformed_call(finish)
        {
            let refusal = (finish == Some("refusal")).then_some("refusal");
            check_schema_answer(&response.content, schema, "mock", refusal, finish)?;
        }
        Ok(response)
    }

    fn lock(&self) -> MutexGuard<'_, Asked> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl AiProvider for MockAiProvider {
    /// Python's class name.
    fn name(&self) -> &'static str {
        "MockAIProvider"
    }

    fn model_name(&self) -> &'static str {
        "mock"
    }

    /// Python's two entries, for code that reads a catalogue.
    fn available_models() -> Vec<ModelInfo> {
        let entry = |id: &str, name: &str, vision: bool| ModelInfo {
            display_name: Some(name.to_owned()),
            context_window: Some(8192),
            supports_vision: Some(vision),
            ..ModelInfo::new(id)
        };
        vec![
            entry("mock", "Mock", false),
            entry("mock-vision", "Mock Vision", true),
        ]
    }

    fn supports_vision(&self) -> bool {
        self.vision
    }

    fn supports_streaming(&self) -> bool {
        self.streaming
    }

    fn supports_structured_streaming(&self) -> bool {
        self.streaming
    }

    fn supports_response_schema(&self) -> bool {
        self.schema.0
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        self.schema.1
    }

    fn generate<'a>(
        &'a self,
        request: &'a AiRequest,
    ) -> impl Future<Output = Result<AiResponse, ProviderError>> + Send + 'a {
        std::future::ready(self.checked_answer(request))
    }

    /// Each response as its events, text first, the way a streaming
    /// provider sends them and Python's mock wraps its `generate` for one
    /// that does not. A streaming mock checks a constrained answer before
    /// its done event, as a real provider's stream does.
    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        let streamed = self.streaming && request.response_schema.is_some();
        let events: ProviderStream<'a> = Box::pin(items(
            move |out: Yielder<Result<StreamEvent, ProviderError>>| async move {
                let response = if streamed {
                    // Unchecked here: the check comes at the done event.
                    self.refuse_unsupported(request)
                        .map(|()| self.answer(request))
                } else {
                    self.checked_answer(request)
                };
                match response {
                    Ok(response) => {
                        for event in response_events(response) {
                            if self.streaming
                                && let StreamEvent::ToolCall(call) = &event
                            {
                                for delta in call_deltas(call, self.tool_call_delta_chunks) {
                                    out.give(Ok(StreamEvent::ToolCallDelta(delta))).await;
                                }
                            }
                            out.give(Ok(event)).await;
                        }
                    }
                    Err(error) => out.give(Err(error)).await,
                }
            },
        ));
        if !streamed {
            return events;
        }
        checked_stream(events, request, "mock", refused)
    }

    /// The text of each response, as one piece, read off its structured
    /// stream, a constrained answer's check included.
    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        let mut events = self.generate_structured_stream(request);
        Box::pin(items(
            move |out: Yielder<Result<String, ProviderError>>| async move {
                while let Some(event) = next(&mut events).await {
                    match event {
                        Ok(StreamEvent::TextDelta(delta)) => out.give(Ok(delta.text)).await,
                        Ok(
                            StreamEvent::ToolCall(_)
                            | StreamEvent::Done(_)
                            | StreamEvent::ThinkingDelta(_)
                            | StreamEvent::ToolCallDelta(_)
                            | StreamEvent::Usage(_),
                        ) => {}
                        Err(error) => return out.give(Err(error)).await,
                    }
                }
            },
        ))
    }
}

/// Python's mock splits `json.dumps(arguments)` before each complete call.
/// The ASCII JSON serialization makes byte boundaries character boundaries.
fn call_deltas(call: &StreamToolCall, chunks: i64) -> Vec<StreamToolCallDelta> {
    let Ok(chunks) = usize::try_from(chunks) else {
        return Vec::new();
    };
    if chunks == 0 {
        return Vec::new();
    }
    let payload = dumps(&Value::Object(call.arguments.clone()));
    let size = payload.len().div_ceil(chunks).max(1);
    payload
        .as_bytes()
        .chunks(size)
        .map(|fragment| StreamToolCallDelta {
            arguments_delta: String::from_utf8_lossy(fragment).into_owned(),
            ..StreamToolCallDelta::new(&call.id, &call.name)
        })
        .collect()
}

/// Why a scripted response declined: its `refusal` ending.
fn refused(done: &StreamDone) -> Option<String> {
    (done.finish_reason.as_deref() == Some("refusal")).then(|| "refusal".to_owned())
}

/// A response saying `text`, ending `stop`, with Python's mock usage.
fn text_response(text: &str) -> AiResponse {
    let mut usage = Metadata::new();
    usage.insert("prompt_tokens".to_owned(), Value::from(10));
    usage.insert("completion_tokens".to_owned(), Value::from(5));
    AiResponse {
        finish_reason: Some("stop".to_owned()),
        usage,
        ..AiResponse::new(text)
    }
}

#[cfg(test)]
mod tests;
