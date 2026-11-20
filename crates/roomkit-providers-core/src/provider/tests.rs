//! The trait's provided methods, on a provider with `generate` alone or a
//! text stream beside it: Python's
//! `tests/test_providers/test_stream_adapter.py` and the base provider's
//! catalogue tests of `tests/test_providers/test_ai_models.py`.

use serde_json::json;

use super::*;
use crate::mock::{MockAiProvider, MockAiProviderOptions};
use crate::request::{AiMessage, AiRole};
use crate::response::{AiToolCall, StreamThinkingDelta, StreamToolCall};
use crate::testing::{block_on, collected};
use crate::tools::AiTool;

/// A provider with `generate` alone, or a text stream beside it: Python's
/// `_Generating`.
struct Generating {
    response: AiResponse,
    streams_text: bool,
}

impl AiProvider for Generating {
    fn model_name(&self) -> &'static str {
        "generating"
    }

    fn supports_streaming(&self) -> bool {
        self.streams_text
    }

    fn generate<'a>(
        &'a self,
        _request: &'a AiRequest,
    ) -> impl Future<Output = Result<AiResponse, ProviderError>> + Send + 'a {
        std::future::ready(Ok(self.response.clone()))
    }

    fn generate_stream<'a>(&'a self, _request: &'a AiRequest) -> TextStream<'a> {
        Box::pin(items(
            |out: Yielder<Result<String, ProviderError>>| async move {
                for text in ["Hel", "lo."] {
                    out.give(Ok(text.to_owned())).await;
                }
            },
        ))
    }
}

fn asking(tools: bool) -> AiRequest {
    let tools = if tools {
        vec![AiTool::new("lookup", "d").unwrap()]
    } else {
        Vec::new()
    };
    AiRequest {
        messages: vec![AiMessage::new(AiRole::User, "hi")],
        tools,
        ..AiRequest::default()
    }
}

fn events(provider: &impl AiProvider, request: &AiRequest) -> Vec<StreamEvent> {
    collected(provider.generate_structured_stream(request))
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap()
}

// Python: tests/test_providers/test_stream_adapter.py::test_a_wrapped_response_keeps_its_thinking_signature_and_call_metadata
#[test]
fn a_wrapped_response_keeps_its_thinking_signature_and_call_metadata() {
    let mut call = AiToolCall::new("c1", "lookup");
    call.arguments.insert("q".into(), json!("x"));
    call.metadata
        .insert("thought_signature".into(), json!("TS"));
    let mut response = AiResponse {
        thinking: Some("I should look it up.".to_owned()),
        thinking_signature: Some("SIG-123".to_owned()),
        finish_reason: Some("tool_calls".to_owned()),
        tool_calls: vec![call.clone()],
        ..AiResponse::new("Looking.")
    };
    response.usage.insert("input_tokens".into(), json!(10));
    response.usage.insert("output_tokens".into(), json!(2));
    response.metadata.insert("model".into(), json!("m"));
    let provider = Generating {
        response: response.clone(),
        streams_text: false,
    };
    assert_eq!(
        events(&provider, &asking(true)),
        [
            StreamEvent::ThinkingDelta(StreamThinkingDelta {
                signature: Some("SIG-123".to_owned()),
                ..StreamThinkingDelta::new("I should look it up.")
            }),
            StreamEvent::TextDelta(StreamTextDelta::new("Looking.")),
            StreamEvent::ToolCall(StreamToolCall::from(call)),
            StreamEvent::Done(StreamDone {
                finish_reason: response.finish_reason,
                usage: response.usage,
                metadata: response.metadata,
            }),
        ]
    );
}

// Python: tests/test_providers/test_stream_adapter.py::test_a_turn_without_tools_streams_the_text_a_provider_streams
#[test]
fn a_turn_without_tools_streams_the_text_a_provider_streams() {
    let provider = Generating {
        response: AiResponse::new("never read"),
        streams_text: true,
    };
    assert_eq!(
        events(&provider, &asking(false)),
        [
            StreamEvent::TextDelta(StreamTextDelta::new("Hel")),
            StreamEvent::TextDelta(StreamTextDelta::new("lo.")),
            StreamEvent::Done(StreamDone {
                finish_reason: Some("stop".to_owned()),
                ..StreamDone::default()
            }),
        ]
    );
}

// Python: tests/test_providers/test_stream_adapter.py::test_a_turn_with_tools_wraps_generate_even_where_text_streams
#[test]
fn a_turn_with_tools_wraps_generate_even_where_text_streams() {
    let provider = Generating {
        response: AiResponse::new("From generate."),
        streams_text: true,
    };
    assert_eq!(
        events(&provider, &asking(true)).first(),
        Some(&StreamEvent::TextDelta(StreamTextDelta::new(
            "From generate."
        )))
    );
}

// Python: AIProvider.generate_stream raises for a provider that does not
// stream text, naming it.
#[test]
fn a_provider_that_does_not_stream_text_fails_its_text_stream() {
    let read = collected(Bare.generate_stream(&asking(false)));
    let [Err(error)] = read.as_slice() else {
        panic!("one failure, got {read:?}");
    };
    assert_eq!(error.message, "Bare does not support streaming generation");
    assert!(!error.retryable);
}

// Python: AIProvider.name is the class name by default; its type name is
// what an object of the provider says too.
#[test]
fn a_provider_is_named_after_its_type_and_held_as_an_object() {
    let provider = Generating {
        response: AiResponse::new("held"),
        streams_text: false,
    };
    assert_eq!(provider.name(), "Generating");
    let held: std::sync::Arc<dyn DynAiProvider> = std::sync::Arc::new(provider);
    assert_eq!(
        (
            held.dyn_name(),
            held.dyn_type_name(),
            held.dyn_model_name(),
            held.dyn_supports_vision(),
            held.dyn_supports_deferred_tools(),
            held.dyn_context_window(),
            held.dyn_catalog_entry(),
        ),
        (
            "Generating",
            "Generating",
            "generating",
            false,
            false,
            None,
            None
        )
    );
    let request = asking(false);
    let read = collected(held.dyn_generate_structured_stream(&request));
    assert_eq!(read.len(), 2, "a text and its end: {read:?}");
    block_on(held.dyn_close());
}

/// A provider no catalogue carries, as Python's `_Bare`.
struct Bare;

impl AiProvider for Bare {
    fn model_name(&self) -> &'static str {
        "some-custom-local-model"
    }

    fn generate<'a>(
        &'a self,
        _request: &'a AiRequest,
    ) -> impl Future<Output = Result<AiResponse, ProviderError>> + Send + 'a {
        std::future::ready(Ok(AiResponse::new("")))
    }
}

// Python: tests/test_providers/test_ai_models.py::test_base_available_models_is_empty
#[test]
fn base_available_models_is_empty() {
    assert_eq!(Bare::available_models(), Vec::new());
}

// Python: tests/test_providers/test_ai_models.py::test_context_window_resolves_from_catalog
#[test]
fn context_window_resolves_from_catalog() {
    assert_eq!(
        MockAiProvider::new(MockAiProviderOptions::default()).context_window(),
        Some(8192)
    );
}

// Python: tests/test_providers/test_ai_models.py::test_context_window_none_when_model_absent
#[test]
fn context_window_none_when_model_absent() {
    assert_eq!(Bare.context_window(), None);
}

// Python: tests/test_providers/test_ai_models.py::test_catalog_entry_returns_the_active_models_metadata
#[test]
fn catalog_entry_returns_the_active_models_metadata() {
    let entry = MockAiProvider::new(MockAiProviderOptions::default())
        .catalog_entry()
        .unwrap();
    assert_eq!(entry.id, "mock");
    assert_eq!(entry.context_window, Some(8192));
}

// Python: tests/test_providers/test_ai_models.py::test_catalog_entry_is_none_for_an_unknown_model
#[test]
fn catalog_entry_is_none_for_an_unknown_model() {
    assert_eq!(Bare.catalog_entry(), None);
}

// Python: tests/test_providers/test_ai_models.py::test_base_list_models_falls_back_to_curated
#[test]
fn base_list_models_falls_back_to_curated() {
    let provider = MockAiProvider::new(MockAiProviderOptions::default());
    assert_eq!(
        block_on(provider.list_models()).unwrap(),
        MockAiProvider::available_models()
    );
}

// Python: tests/test_providers/test_ai_models.py::test_mock_catalog
#[test]
fn mock_catalog() {
    let ids: Vec<String> = MockAiProvider::available_models()
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    assert_eq!(ids, ["mock", "mock-vision"]);
}

// Python: request_api_key: an absent entry, an empty string and a value
// that is no string are no credential.
#[test]
fn a_request_s_credential_is_a_non_empty_string_under_its_key() {
    let with = |value: Value| {
        let mut request = AiRequest::default();
        request.metadata.insert(API_KEY_METADATA_KEY.into(), value);
        request
    };
    assert_eq!(request_api_key(&AiRequest::default()), None);
    assert_eq!(request_api_key(&with(json!(""))), None);
    assert_eq!(request_api_key(&with(json!(42))), None);
    assert_eq!(
        request_api_key(&with(json!("placeholder-not-a-key"))),
        Some("placeholder-not-a-key")
    );
    // A request that crossed as JSON holds the mask, not the key.
    let crossed: AiRequest =
        serde_json::from_value(serde_json::to_value(with(json!("placeholder-not-a-key"))).unwrap())
            .unwrap();
    assert_eq!(request_api_key(&crossed), Some("**********"));
}
