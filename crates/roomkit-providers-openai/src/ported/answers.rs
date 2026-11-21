//! What the provider reads of a whole answer: its calls, its usage, its
//! reasoning, and an answer constrained by a response schema.

use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, Metadata, ProviderError, ResponseSchemaFailure,
};
use serde_json::{Map, Value, json};

use super::support::{
    KEY, asked, chunk, completion, elsewhere, events, hello, official, pointed, saying,
    schema_failure, streamed,
};
use crate::failure::test_exit;
use crate::replay::fake::Fake;
use crate::{OpenAi, OpenAiConfig};

/// What the provider reads of a 200 JSON answer whose body is `body`, a
/// failure as it leaves the provider.
fn read(body: &Value) -> Result<AiResponse, ProviderError> {
    let body = body.to_string();
    let read = crate::answer::read(body.as_bytes(), true, &hello(), "asked-model", "openai");
    read.map_err(|failure| test_exit().leave(&failure))
}

/// A completion's body whose one choice holds `message`, with `usage`.
fn completion_body(message: &Value, usage: &Value) -> Value {
    json!({
        "choices": [{"index": 0, "message": message, "finish_reason": "tool_calls"}],
        "usage": usage,
    })
}

// Source: src/wire.rs::tests::a_completion_s_nulls_read_as_absent
#[test]
fn a_completion_s_nulls_read_as_absent() {
    let body = json!({
        "id": null,
        "choices": [{"index": 0, "finish_reason": null, "message": {
            "role": "assistant", "content": null, "refusal": null,
            "tool_calls": [{"id": null, "type": "function",
                "function": {"name": "lookup", "arguments": null}}],
        }}],
        "usage": {"prompt_tokens": null, "completion_tokens": 7, "total_tokens": null,
            "prompt_tokens_details": {"cached_tokens": null}},
    });
    let response = read(&body).unwrap();
    let [call] = response.tool_calls.as_slice() else {
        panic!("one call: {:?}", response.tool_calls);
    };
    // A call its server gave no id takes one of the provider's.
    assert!(call.id.starts_with("call_lookup_"), "{}", call.id);
    assert_eq!(
        (call.name.as_str(), &call.arguments),
        ("lookup", &Metadata::new())
    );
    assert_eq!(Value::Object(response.usage), json!({"output_tokens": 7}));
    assert_eq!(
        (response.content.as_str(), response.finish_reason),
        ("", None)
    );
}

// Source: src/wire.rs::tests::a_count_the_server_leaves_out_stays_unknown
// (the write reported apart, which no replayed case holds)
#[test]
fn a_cache_write_is_kept_where_the_prompt_s_count_is_left_out() {
    let usage = json!({"completion_tokens": 7, "prompt_tokens_details": {"cache_write_tokens": 5}});
    let response = read(&completion_body(&json!({"content": "Hi"}), &usage)).unwrap();
    assert_eq!(
        Value::Object(response.usage),
        json!({"output_tokens": 7, "cache_creation_input_tokens": 5})
    );
}

// Source: src/wire.rs::tests::a_whole_answer_s_entry_with_no_function_is_no_call
#[test]
fn a_whole_answer_s_entry_with_no_function_is_no_call() {
    let message = json!({"tool_calls": [
        {"id": "a", "type": "custom", "custom": {"name": "x"}},
        {"id": "b", "function": {"name": "now", "arguments": "{}"}},
    ]});
    let response = read(&completion_body(&message, &Value::Null)).unwrap();
    let calls: Vec<(&str, &str, &Metadata)> = response
        .tool_calls
        .iter()
        .map(|call| (call.id.as_str(), call.name.as_str(), &call.arguments))
        .collect();
    assert_eq!(calls, [("b", "now", &Metadata::new())]);
}

// Source: src/wire.rs::tests::a_malformed_answer_s_reason_holds_no_key
// (there the typed reader quotes the answer for choices that are no list;
// here choices that are no list fail in words of the provider's own, and
// what quotes the answer is an error object with no message, written whole,
// and the reader's refusal of a streamed call's index)
#[tokio::test]
async fn a_malformed_answer_s_reason_holds_no_key() {
    let object = json!({"error": {"detail": format!("the key {KEY} is refused")}});
    let error = read(&object).unwrap_err();
    assert_eq!(error.message, r#"{"detail":"the key [***] is refused"}"#);
    let call = json!({"tool_calls": [{"index": KEY, "function": {"name": "now"}}]});
    let (_, failure) = streamed(events(&[chunk(&call)]), &hello()).await;
    let error = failure.unwrap();
    assert_eq!(
        error.message,
        "invalid type: string \"[***]\", expected u64"
    );
    for error in [read(&json!({"choices": KEY})).unwrap_err(), error] {
        let printed = format!("{error} {error:?}");
        assert!(!printed.contains(KEY), "{printed}");
    }
}

// Not in Python, which reads each of these as an empty answer: a 200 whose
// body is JSON and no completion is no answer. A completion holds its
// choices as a list, an empty one included.
#[tokio::test]
async fn a_json_body_that_is_no_completion_is_a_failure_not_an_empty_answer() {
    let no_completion = [
        json!({"status": "ok"}),
        json!({}),
        json!({"choices": null}),
        json!({"id": "chatcmpl-1", "object": "chat.completion", "model": "answering-model"}),
        json!({"error": null, "usage": {"prompt_tokens": 12}}),
    ];
    for body in no_completion {
        let error = asked(super::support::json_reply(200, body.to_string()), &hello()).await;
        let error = error.unwrap_err();
        assert_eq!(
            error.message,
            "The provider answered with a body it could not read: not a chat completion",
            "{body}"
        );
        let attributes = (error.retryable, error.status_code, error.before_request);
        assert_eq!(attributes, (false, None, false), "{body}");
    }
    // A completion with no choice is one: an answer that says nothing.
    let empty = json!({"choices": [], "usage": {"prompt_tokens": 12}});
    let response = read(&empty).unwrap();
    assert_eq!(response.content, "");
    assert_eq!(
        serde_json::to_string(&response.usage).unwrap(),
        r#"{"input_tokens":12}"#
    );
}

// A body that is no completion and words why, as a message or a detail:
// the failure carries those words, bounded and cleaned as any other.
#[tokio::test]
async fn a_body_that_is_no_completion_says_what_the_body_says() {
    let said = "The provider answered with a body it could not read: not a chat completion";
    let long = "x".repeat(4 * crate::failure::MESSAGE_LIMIT);
    let cases = [
        (
            json!({"message": "The model\n is loading."}),
            format!("{said}: The model is loading."),
        ),
        (
            json!({"detail": "Not found", "status": 404}),
            format!("{said}: Not found"),
        ),
        (
            json!({"detail": format!("key {KEY}")}),
            format!("{said}: key [***]"),
        ),
        (
            json!({"message": "Loading.", "detail": "Wait."}),
            format!("{said}: Loading."),
        ),
        (json!({"detail": [{"msg": "required"}]}), said.to_owned()),
        (json!({"message": ""}), said.to_owned()),
    ];
    for (body, expected) in cases {
        let error = asked(super::support::json_reply(200, body.to_string()), &hello()).await;
        assert_eq!(error.unwrap_err().message, expected, "{body}");
    }
    let wordy = super::support::json_reply(200, json!({"detail": long}).to_string());
    let wordy = asked(wordy, &hello()).await.unwrap_err().message;
    assert_eq!(wordy.chars().count(), crate::failure::MESSAGE_LIMIT);
}

// Python's JSON reader takes a body that starts with a byte order mark: a
// whole answer and the body of a refusal are read past it.
#[tokio::test]
async fn a_byte_order_mark_before_a_whole_body_is_read_past() {
    let marked = |body: Value| format!("\u{feff}{body}");
    let answered = completion_body(&json!({"role": "assistant", "content": "Yes."}), &json!({}));
    let response = asked(super::support::json_reply(200, marked(answered)), &hello()).await;
    assert_eq!(response.unwrap().content, "Yes.");
    let refusal =
        json!({"error": {"message": "Incorrect API key", "type": "invalid_request_error"}});
    let error = asked(super::support::json_reply(401, marked(refusal)), &hello()).await;
    assert_eq!(
        error.unwrap_err().message,
        "the provider answered HTTP 401 (invalid_request_error): Incorrect API key"
    );
    let object = json!({"error": {"message": "Overloaded", "code": 503}});
    let error = asked(super::support::json_reply(200, marked(object)), &hello()).await;
    assert_eq!(error.unwrap_err().status_code, Some(503));
}

/// What `generate` reads of a completion whose message is `message`: its
/// reasoning, its text.
async fn generated(message: &Value) -> (Option<String>, String) {
    let response = asked(completion(message, "stop"), &hello()).await.unwrap();
    (response.thinking, response.content)
}

// Source: tests/agent/thinking.rs::a_think_block_reads_the_same_streamed_or_not
// (the answer asked whole)
#[tokio::test]
async fn a_think_block_and_its_answer_keep_their_spaces() {
    for (content, reasoning, answer) in [
        (
            "<think> weighing it \n</think>\n\nThe answer.  ",
            " weighing it \n",
            "\n\nThe answer.  ",
        ),
        ("<think>x</think>\n\n", "x", "\n\n"),
    ] {
        let whole = generated(&json!({"content": content})).await;
        assert_eq!(whole, (Some(reasoning.to_owned()), answer.to_owned()));
    }
}

// Source: tests/agent/thinking.rs::a_think_tag_the_answer_writes_is_answer
// (the answer asked whole)
#[tokio::test]
async fn a_think_tag_the_answer_writes_is_answer() {
    let said = "The parser reads the <think> tag at the start. Everything after is lost.";
    for reasoning in [None, Some("why")] {
        let mut message = json!({"content": said});
        if let Some(reasoning) = reasoning {
            message["reasoning_content"] = json!(reasoning);
        }
        let whole = generated(&message).await;
        assert_eq!(whole, (reasoning.map(str::to_owned), said.to_owned()));
    }
}

/// The schema of a verdict.
fn verdict() -> Map<String, Value> {
    let schema = json!({
        "type": "object",
        "properties": {"label": {"type": "string"}},
        "required": ["label"],
        "additionalProperties": false,
    });
    schema.as_object().cloned().unwrap_or_default()
}

/// A question under the verdict's schema.
fn asking() -> AiRequest {
    AiRequest {
        response_schema: Some(verdict()),
        ..hello()
    }
}

// Source: tests/agent/response_schema.rs::a_server_declared_without_support_is_refused_before_the_call
#[tokio::test]
async fn a_server_declared_without_support_is_refused_before_the_call() {
    let config = OpenAiConfig {
        supports_response_schema: Some(false),
        ..elsewhere("local-model")
    };
    let fake = Fake::serving(vec![saying(r#"{"label": "yes"}"#)]).await;
    let provider = pointed(config, &fake);
    assert!(!provider.supports_response_schema());
    let error = provider.generate(&asking()).await.unwrap_err();
    assert_eq!(
        schema_failure(&error),
        Some(ResponseSchemaFailure::Unsupported)
    );
    assert_eq!(fake.seen().len(), 0, "no request is sent");
}

// Source: tests/agent/response_schema.rs::on_for_the_official_endpoint_off_behind_a_base_url
#[test]
fn on_for_the_official_endpoint_off_behind_a_base_url() {
    let with_tools = |base_url: Option<&str>, stated: Option<bool>| {
        let config = OpenAiConfig {
            base_url: base_url.map(str::to_owned),
            supports_response_schema_with_tools: stated,
            ..official("gpt-6-luna")
        };
        let provider = OpenAi::new(config).unwrap();
        provider.supports_response_schema_with_tools()
    };
    assert_eq!(
        (
            with_tools(None, None),
            with_tools(Some("http://vllm:8000/v1"), None),
            with_tools(Some("http://proxy/v1"), Some(true)),
        ),
        (true, false, true)
    );
}

// Source: tests/agent/response_schema.rs::entries_with_no_function_are_no_tool_round
// (the answer asked whole)
#[tokio::test]
async fn entries_with_no_function_are_no_tool_round() {
    let entry = json!({"id": "c1", "type": "custom", "custom": {"name": "x", "input": ""}});
    let message = json!({"role": "assistant", "content": "", "tool_calls": [entry]});
    let answered = asked(completion(&message, "tool_calls"), &asking()).await;
    assert_eq!(
        schema_failure(&answered.unwrap_err()),
        Some(ResponseSchemaFailure::InvalidJson)
    );
}

// Source: tests/agent/response_schema.rs::a_refusal_that_is_not_text_is_not_reported
#[tokio::test]
async fn a_refusal_that_is_not_text_is_not_reported() {
    let message = json!({"role": "assistant", "content": null, "refusal": true});
    let response = asked(completion(&message, "stop"), &hello()).await.unwrap();
    assert_eq!(response.metadata.get("refusal"), None);
}

// A call is cut by how the answer stopped only when it is the answer's
// last: with no arguments under a stop for length, the last call must not
// run, and the one before it is a call with no arguments.
#[test]
fn only_the_last_call_of_an_answer_is_cut_by_its_ending() {
    let call = |id: &str| json!({"id": id, "type": "function", "function": {"name": "now", "arguments": ""}});
    let message = json!({"role": "assistant", "content": null, "tool_calls": [call("a"), call("b"), call("c")]});
    let body = json!({"choices": [{"index": 0, "message": message, "finish_reason": "length"}]});
    let response = read(&body).unwrap();
    let cut: Vec<bool> = response
        .tool_calls
        .iter()
        .map(|call| call.partial)
        .collect();
    assert_eq!(cut, [false, false, true]);
}
