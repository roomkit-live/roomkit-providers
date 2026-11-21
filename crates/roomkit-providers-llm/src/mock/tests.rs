//! The scripted provider, as Python's `MockAIProvider`: its answers, and a
//! response schema's contract as it keeps it (RFC §6.7), Python's
//! `tests/test_response_schema.py::TestMockHonoursTheContract`.

use std::error::Error;

use serde_json::{Map, json};

use super::*;
use crate::request::{AiMessage, AiRole};
use crate::response::AiToolCall;
use crate::schema::{ResponseSchemaError, ResponseSchemaErrorOptions, ResponseSchemaFailure};
use crate::testing::{block_on, collected};
use crate::tools::AiTool;

/// Python's `VERDICT`, its root's members.
fn verdict() -> Map<String, Value> {
    let Value::Object(schema) = json!({
        "type": "object",
        "properties": {
            "label": { "type": "string", "enum": ["yes", "no"] },
            "confidence": { "type": "number" },
        },
        "required": ["label", "confidence"],
        "additionalProperties": false,
    }) else {
        unreachable!("an object")
    };
    schema
}

/// Python's `ANSWER`.
const ANSWER: &str = r#"{"label": "yes", "confidence": 0.9}"#;

/// A question under `schema`, with `tools`: Python's `_context`.
fn asking(schema: Option<Map<String, Value>>, tools: Vec<AiTool>) -> AiRequest {
    AiRequest {
        messages: vec![AiMessage::new(AiRole::User, "Is it?")],
        response_schema: schema,
        tools,
        ..AiRequest::default()
    }
}

/// A mock answering `responses`, taking a schema, streaming or not.
fn mock(responses: Vec<AiResponse>, response_schema: bool, streaming: bool) -> MockAiProvider {
    MockAiProvider::new(MockAiProviderOptions {
        ai_responses: Some(responses),
        response_schema,
        streaming,
        ..MockAiProviderOptions::default()
    })
}

/// A response saying `text`, ending `finish`.
fn said(text: &str, finish: &str) -> AiResponse {
    AiResponse {
        finish_reason: Some(finish.to_owned()),
        ..AiResponse::new(text)
    }
}

/// The schema's failure `error` carries, when one failed.
fn reason(error: &ProviderError) -> Option<ResponseSchemaFailure> {
    let failure = error.source()?.downcast_ref::<ResponseSchemaError>()?;
    Some(failure.reason)
}

/// The events of `provider`'s stream for `request`, up to its end or its
/// first failure.
fn drained(
    provider: &MockAiProvider,
    request: &AiRequest,
) -> (Vec<StreamEvent>, Option<ProviderError>) {
    let mut events = Vec::new();
    for event in collected(provider.generate_structured_stream(request)) {
        match event {
            Ok(event) => events.push(event),
            Err(error) => return (events, Some(error)),
        }
    }
    (events, None)
}

/// Python's mock: its texts round-robin, `Hello from AI` without any, whole
/// responses in their place when given; it keeps each request, and names
/// itself `MockAIProvider`, its model `mock`.
#[test]
fn the_mock_answers_its_responses_round_robin_and_keeps_what_it_was_asked() {
    let cases = [
        (
            vec!["One.".to_owned(), "Two.".to_owned()],
            None,
            ["One.", "Two.", "One."],
        ),
        (Vec::new(), None, ["Hello from AI"; 3]),
        (
            vec!["unused".to_owned()],
            Some(vec![AiResponse::new("Whole.")]),
            ["Whole."; 3],
        ),
    ];
    for (responses, ai_responses, expected) in cases {
        for streaming in [false, true] {
            let model = MockAiProvider::new(MockAiProviderOptions {
                responses: responses.clone(),
                ai_responses: ai_responses.clone(),
                streaming,
                ..MockAiProviderOptions::default()
            });
            let kept = model.clone();
            let answers: Vec<String> = ["Hi?", "Again?", "Once more?"]
                .into_iter()
                .map(|question| {
                    let request = AiRequest {
                        messages: vec![AiMessage::new(AiRole::User, question)],
                        ..AiRequest::default()
                    };
                    block_on(model.generate(&request)).unwrap().content
                })
                .collect();
            let asked: Vec<String> = kept
                .calls()
                .iter()
                .filter_map(|request| request.messages.first().map(AiMessage::text))
                .collect();
            assert_eq!(
                (
                    answers,
                    asked,
                    model.name(),
                    model.model_name(),
                    model.supports_streaming(),
                ),
                (
                    expected.map(str::to_owned).to_vec(),
                    vec![
                        "Hi?".to_owned(),
                        "Again?".to_owned(),
                        "Once more?".to_owned()
                    ],
                    "MockAIProvider",
                    "mock",
                    streaming
                ),
                "{responses:?} {streaming}"
            );
        }
    }
}

// Python: MockAIProvider's default response: its text, ending `stop`, with
// its usage; its text stream gives the text as one piece.
#[test]
fn a_scripted_text_ends_stop_with_the_mock_s_usage() {
    let model = MockAiProvider::new(MockAiProviderOptions {
        responses: vec!["Shipped.".to_owned()],
        streaming: true,
        ..MockAiProviderOptions::default()
    });
    let request = asking(None, Vec::new());
    let response = block_on(model.generate(&request)).unwrap();
    assert_eq!(
        serde_json::to_value(&response).unwrap(),
        json!({
            "content": "Shipped.",
            "thinking": null,
            "thinking_signature": null,
            "thinking_parts": null,
            "finish_reason": "stop",
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 },
            "metadata": {},
            "tool_calls": [],
        })
    );
    let text: Vec<String> = collected(model.generate_stream(&request))
        .into_iter()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(text, ["Shipped."]);
}

// Python: MockAIProvider(tool_call_delta_chunks=n): a streaming mock splits
// `json.dumps(arguments)` into that many fragments before the complete call,
// each naming its call; no fragment for a count of zero or less, nor from a
// mock that does not stream.
#[test]
fn mock_fragments_keep_python_json_and_complete_call() {
    // Python's json.dumps writes ASCII escapes, spaces and surrogate pairs.
    let payload = r#"{"q": "\u00e9\u732b\ud83d\ude42"}"#;
    for streaming in [false, true] {
        for chunks in [-1, 0, 1, 4, 100] {
            let arguments = json!({ "q": "\u{e9}\u{732b}\u{1f642}" });
            let arguments = arguments.as_object().cloned().unwrap_or_default();
            let scripted = AiResponse {
                tool_calls: vec![AiToolCall {
                    arguments: arguments.clone(),
                    ..AiToolCall::new("c1", "lookup")
                }],
                ..AiResponse::new("")
            };
            let model = MockAiProvider::new(MockAiProviderOptions {
                streaming,
                tool_call_delta_chunks: chunks,
                ai_responses: Some(vec![scripted]),
                ..MockAiProviderOptions::default()
            });
            let (events, failed) = drained(&model, &AiRequest::default());
            assert!(failed.is_none());
            let mut fragments = Vec::new();
            let mut calls = Vec::new();
            for event in events {
                match event {
                    StreamEvent::ToolCallDelta(delta) => {
                        assert_eq!((&*delta.id, &*delta.name, delta.index), ("c1", "lookup", 0));
                        // A fragment comes before its complete call.
                        assert_eq!(calls.len(), 0);
                        fragments.push(delta.arguments_delta);
                    }
                    StreamEvent::ToolCall(call) => calls.push(call),
                    StreamEvent::TextDelta(_)
                    | StreamEvent::Done(_)
                    | StreamEvent::ThinkingDelta(_)
                    | StreamEvent::Usage(_) => {}
                }
            }
            let expected: Vec<String> = match usize::try_from(chunks) {
                Ok(count) if streaming && count > 0 => payload
                    .as_bytes()
                    .chunks(payload.len().div_ceil(count))
                    .map(|fragment| String::from_utf8(fragment.to_vec()).unwrap())
                    .collect(),
                Ok(_) | Err(_) => Vec::new(),
            };
            assert_eq!(
                fragments, expected,
                "streaming={streaming}, chunks={chunks}"
            );
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].arguments, arguments);
        }
    }
}

// Python: MockAIProvider's properties: each says what its keyword said.
#[test]
fn the_mock_says_of_itself_what_its_options_say() {
    let said = |options: MockAiProviderOptions| {
        let model = MockAiProvider::new(options);
        [
            model.supports_vision(),
            model.supports_streaming(),
            model.supports_structured_streaming(),
            model.supports_response_schema(),
            model.supports_response_schema_with_tools(),
        ]
    };
    let options = MockAiProviderOptions::default;
    assert_eq!(said(options()), [false; 5]);
    assert_eq!(
        said(MockAiProviderOptions {
            vision: true,
            ..options()
        }),
        [true, false, false, false, false]
    );
    assert_eq!(
        said(MockAiProviderOptions {
            streaming: true,
            ..options()
        }),
        [false, true, true, false, false]
    );
    assert_eq!(
        said(MockAiProviderOptions {
            response_schema: true,
            ..options()
        }),
        [false, false, false, true, false]
    );
    assert_eq!(
        said(MockAiProviderOptions {
            response_schema_with_tools: true,
            ..options()
        }),
        [false, false, false, false, true]
    );
}

// RFC §6.4: a response that calls a tool is no answer. Under a schema, a
// provider that takes one beside tools hands the calls on, its text
// unchecked, whole or streamed.
#[test]
fn a_constrained_round_that_calls_a_tool_is_not_checked_as_an_answer() {
    let calling = AiResponse {
        finish_reason: Some("tool_calls".to_owned()),
        tool_calls: vec![AiToolCall::new("c1", "lookup")],
        ..AiResponse::new("Let me check.")
    };
    let tool = AiTool::new("lookup", "Look it up").unwrap();
    let request = asking(Some(verdict()), vec![tool]);
    for streaming in [false, true] {
        let provider = MockAiProvider::new(MockAiProviderOptions {
            ai_responses: Some(vec![calling.clone()]),
            response_schema: true,
            response_schema_with_tools: true,
            streaming,
            ..MockAiProviderOptions::default()
        });
        let response = block_on(provider.generate(&request)).expect("a round of calls");
        assert_eq!(response.tool_calls.len(), 1);
        let (events, failed) = drained(&provider, &request);
        assert_eq!(failed, None, "streaming={streaming}");
        assert!(matches!(
            events.as_slice(),
            [
                StreamEvent::TextDelta(_),
                StreamEvent::ToolCall(_),
                StreamEvent::Done(_)
            ]
        ));
    }
}

// RFC §6.4: a response that ended on a call its provider could not hand
// over is no answer either: its empty text is not refused as no document.
#[test]
fn a_constrained_round_that_ends_on_a_malformed_call_is_not_checked_as_an_answer() {
    let request = asking(Some(verdict()), Vec::new());
    for streaming in [false, true] {
        for ending in ["MALFORMED_FUNCTION_CALL", "unexpected_tool_call"] {
            let provider = mock(vec![said("", ending)], true, streaming);
            let response = block_on(provider.generate(&request)).expect("no answer to check");
            assert_eq!(response.finish_reason.as_deref(), Some(ending));
            let (events, failed) = drained(&provider, &request);
            assert_eq!(failed, None, "streaming={streaming} {ending}");
            assert!(matches!(events.as_slice(), [StreamEvent::Done(_)]));
        }
    }
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_generate_returns_the_json_document
#[test]
fn generate_returns_the_json_document() {
    let provider = mock(vec![said(ANSWER, "stop")], true, false);
    let response = block_on(provider.generate(&asking(Some(verdict()), Vec::new())));
    let response = response.expect("the document satisfies the schema");
    let document: Value = serde_json::from_str(&response.content).expect("JSON");
    assert_eq!(document["label"], "yes");
    assert_eq!(provider.calls()[0].response_schema, Some(verdict()));
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_a_provider_without_support_refuses_before_sending
#[test]
fn a_provider_without_support_refuses_before_sending() {
    let provider = mock(vec![said(ANSWER, "stop")], false, false);
    let error =
        block_on(provider.generate(&asking(Some(verdict()), Vec::new()))).expect_err("refused");
    assert_eq!(reason(&error), Some(ResponseSchemaFailure::Unsupported));
    assert!(error.before_request);
    assert_eq!(provider.calls().len(), 0, "no request kept");
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_tools_in_the_same_turn_are_refused
#[test]
fn tools_in_the_same_turn_are_refused() {
    let provider = mock(vec![said(ANSWER, "stop")], true, false);
    let tool = AiTool::new("lookup", "Look it up").expect("a name every vendor takes");
    let error =
        block_on(provider.generate(&asking(Some(verdict()), vec![tool]))).expect_err("refused");
    assert_eq!(reason(&error), Some(ResponseSchemaFailure::Unsupported));
    assert!(error.to_string().contains("tools"), "{error}");
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_streaming_carries_the_schema_and_checks_the_answer
#[test]
fn streaming_carries_the_schema_and_checks_the_answer() {
    let provider = mock(
        vec![said(ANSWER, "stop"), said("Sure.", "stop")],
        true,
        true,
    );
    let asked = asking(Some(verdict()), Vec::new());
    let (streamed, failed) = drained(&provider, &asked);
    let (provisional, refused) = drained(&provider, &asked);
    assert!(failed.is_none());
    assert!(matches!(streamed.last(), Some(StreamEvent::Done(_))));
    let refused = refused.expect("the second answer is no document");
    assert_eq!(reason(&refused), Some(ResponseSchemaFailure::InvalidJson));
    // The text went out as it came; the error stands in for the done event.
    assert!(matches!(
        provisional.as_slice(),
        [StreamEvent::TextDelta(_)]
    ));
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_an_answer_without_its_document_raises
#[test]
fn an_answer_without_its_document_raises() {
    let cases = [
        (said("", "refusal"), ResponseSchemaFailure::Refusal),
        (
            said(r#"{"label": "y"#, "length"),
            ResponseSchemaFailure::Truncated,
        ),
        (
            said(r#"{"label": "y"#, "model_length"),
            ResponseSchemaFailure::Truncated,
        ),
        (
            said("Sure, the answer is yes.", "stop"),
            ResponseSchemaFailure::InvalidJson,
        ),
        (
            said(r#"{"verdict": "yes"}"#, "stop"),
            ResponseSchemaFailure::InvalidJson,
        ),
    ];
    for (scripted, expected) in cases {
        let provider = mock(vec![scripted], true, false);
        let error = block_on(provider.generate(&asking(Some(verdict()), Vec::new())))
            .expect_err("no document");
        assert_eq!(reason(&error), Some(expected), "{error}");
    }
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_prose_without_a_schema_is_left_alone
#[test]
fn prose_without_a_schema_is_left_alone() {
    let provider = mock(vec![said("Sure, the answer is yes.", "stop")], true, false);
    let response = block_on(provider.generate(&asking(None, Vec::new())));
    assert_eq!(response.expect("prose").content, "Sure, the answer is yes.");
}

// Python: tests/test_response_schema.py::TestMockHonoursTheContract::test_the_error_is_a_provider_error_never_retried
#[test]
fn the_error_is_a_provider_error_never_retried() {
    let options = ResponseSchemaErrorOptions {
        provider: "mock".to_owned(),
    };
    let failure = ResponseSchemaError::new("x", ResponseSchemaFailure::Refusal, options);
    let error = ProviderError::from(failure);
    // Only an unsupported schema is refused before any request.
    assert_eq!(
        (
            error.retryable,
            error.provider.as_str(),
            reason(&error),
            error.before_request
        ),
        (false, "mock", Some(ResponseSchemaFailure::Refusal), false)
    );
}

// Python: tests/test_response_schema.py::TestAIContextField::test_refuses_a_non_portable_schema_at_construction
// A request built in Rust runs no check: the provider refuses the schema
// before any request, in the check's words.
#[test]
fn a_schema_outside_the_subset_is_refused_before_any_request() {
    let provider = mock(vec![said(ANSWER, "stop")], true, false);
    let mut open = verdict();
    open.insert("additionalProperties".to_owned(), Value::Bool(true));
    let error = block_on(provider.generate(&asking(Some(open), Vec::new()))).expect_err("refused");
    assert!(
        error.to_string().contains("additionalProperties"),
        "{error}"
    );
    // Refused before any request: it refuses none of the turn's images.
    assert!(error.before_request);
    assert_eq!(provider.calls().len(), 0, "no request kept");
}
