//! What the provider reads of a whole answer: its calls, its usage, its
//! reasoning, and an answer constrained by a response schema.

use roomkit_providers_core::{
    AiProvider, AiRequest, AiResponse, Metadata, ProviderError, ResponseSchemaFailure,
};
use serde_json::{Map, Value, json};

use super::support::{
    KEY, asked, completion, elsewhere, hello, official, pointed, saying, schema_failure,
};
use crate::failure::Failing;
use crate::replay::fake::Fake;
use crate::{OpenAiAiProvider, OpenAiConfig};

const FAILING: Failing<'static> = Failing {
    provider: "openai",
    secret: KEY,
};

/// What the provider reads of a 200 JSON answer whose body is `body`.
fn read(body: &Value) -> Result<AiResponse, ProviderError> {
    let body = body.to_string();
    crate::answer::read(body.as_bytes(), true, &hello(), "asked-model", FAILING)
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
#[test]
fn a_malformed_answer_s_reason_holds_no_key() {
    let error = read(&json!({"choices": KEY})).unwrap_err();
    let printed = format!("{error} {error:?}");
    assert!(!printed.contains(KEY), "{printed}");
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
        let provider = OpenAiAiProvider::new(config).unwrap();
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
