//! What a request carries: its token bound, the switches of its
//! configuration, and its conversation as Chat Completions messages.

use roomkit_providers_core::{
    AiContentPart, AiImagePart, AiMessage, AiRequest, AiRole, AiTextPart, AiThinkingPart,
    AiToolCallPart, AiToolResultPart, ToolResult, ToolResultPart,
};
use serde_json::{Value, json};

use super::support::{KEY, body_sent, elsewhere, hello};
use crate::OpenAiConfig;
use crate::messages::chat_messages;

// Source: src/wire.rs::tests::no_cap_is_sent_when_none_is_set
#[tokio::test]
async fn no_cap_is_sent_when_none_is_set() {
    let mut sent = Vec::new();
    for (completion_tokens, turn) in [
        (false, None),
        (false, Some(321)),
        (true, None),
        (true, Some(321)),
    ] {
        let config = OpenAiConfig {
            max_tokens: None,
            use_max_completion_tokens: Some(completion_tokens),
            ..elsewhere("local-model")
        };
        let request = AiRequest {
            max_tokens: turn,
            ..hello()
        };
        let body = body_sent(config, &request).await;
        sent.push((
            body.get("max_tokens").cloned(),
            body.get("max_completion_tokens").cloned(),
        ));
    }
    assert_eq!(
        sent,
        [
            (None, None),
            (Some(json!(321)), None),
            (None, None),
            (None, Some(json!(321))),
        ]
    );
}

/// Whether a request for `model` at `base_url` carries its bound as
/// `max_completion_tokens`, and whether it carries a temperature: the
/// switches of the configuration `switches`, each set or left to the
/// profile.
async fn shape(
    base_url: Option<&str>,
    model: &str,
    switches: (Option<bool>, Option<bool>),
) -> (bool, bool) {
    let config = OpenAiConfig {
        base_url: base_url.map(str::to_owned),
        use_max_completion_tokens: switches.0,
        supports_custom_temperature: switches.1,
        ..OpenAiConfig::new(KEY, model)
    };
    let body = body_sent(config, &hello()).await;
    (
        body.get("max_completion_tokens").is_some(),
        body.get("temperature").is_some(),
    )
}

// Source: src/wire.rs::tests::a_modern_model_on_openai_s_endpoint_takes_python_s_profile
#[tokio::test]
async fn a_modern_model_on_openai_s_endpoint_takes_python_s_profile() {
    let unset = (None, None);
    assert_eq!(
        [
            shape(None, "gpt-6-luna", unset).await,
            shape(Some("https://API.openai.com:443/v1/"), "o3-mini", unset).await,
            shape(None, "gpt-4.1-mini", unset).await,
            shape(None, "gpt-4o", unset).await,
            shape(Some("http://localhost:8000/v1"), "gpt-6-luna", unset).await,
            shape(None, "gpt-5-chat-latest", (None, Some(true))).await,
            shape(None, "gpt-4.1-mini", (Some(false), None)).await,
            shape(None, "gpt-4o", (Some(true), Some(false))).await,
        ],
        [
            (true, false),
            (true, false),
            (true, true),
            (false, true),
            (false, true),
            (true, true),
            (false, true),
            (true, false),
        ]
    );
}

fn thought() -> AiContentPart {
    AiContentPart::Thinking(AiThinkingPart::new("why"))
}

fn call_part() -> AiContentPart {
    AiContentPart::ToolCall(AiToolCallPart::new("c1", "lookup"))
}

/// The call of [`call_part`] as a message carries it.
fn call_row() -> Value {
    json!({"id": "c1", "type": "function", "function": {"name": "lookup", "arguments": "{}"}})
}

/// `messages` as the endpoint reads them, no field named for a model's
/// earlier reasoning.
fn rendered(messages: &[AiMessage]) -> Value {
    Value::Array(chat_messages(messages, None, None, "openai").unwrap())
}

// Source: src/request.rs::tests::a_round_sends_its_text_and_its_calls_and_no_reasoning
#[test]
fn a_round_sends_its_text_and_its_calls_and_no_reasoning() {
    let say = || AiContentPart::Text(AiTextPart::new("what"));
    let signed = AiContentPart::Thinking(AiThinkingPart {
        signature: Some("s".to_owned()),
        ..AiThinkingPart::new("")
    });
    let cases = [
        (vec![say(), thought(), call_part()], json!("what")),
        (vec![thought(), call_part()], Value::Null),
        (vec![say(), call_part()], json!("what")),
        (vec![signed, call_part()], Value::Null),
    ];
    for (parts, said) in cases {
        let round = AiMessage::new(AiRole::Assistant, parts);
        assert_eq!(
            rendered(&[round]),
            json!([{"role": "assistant", "content": said, "tool_calls": [call_row()]}])
        );
    }
}

// Source: roomkit-ai tests/chat_messages.rs::a_call_wins_over_results_and_keeps_python_argument_text
#[test]
fn a_call_wins_over_results_and_keeps_python_argument_text() {
    let mut call = AiToolCallPart::new("c1", "lookup");
    call.arguments.insert("q".to_owned(), json!("café"));
    call.arguments.insert("n".to_owned(), json!(42));
    for role in [AiRole::Assistant, AiRole::Tool] {
        let message = AiMessage::new(
            role,
            vec![
                AiContentPart::Thinking(AiThinkingPart::new("hidden")),
                AiContentPart::Text(AiTextPart::new("looking")),
                AiContentPart::ToolCall(call.clone()),
                AiContentPart::ToolResult(AiToolResultPart::new("c1", "lookup", "done")),
            ],
        );
        assert_eq!(
            rendered(&[message]),
            json!([{"role": "assistant", "content": "looking", "tool_calls": [{
                "id": "c1",
                "type": "function",
                "function": {"name": "lookup", "arguments": "{\"q\": \"caf\\u00e9\", \"n\": 42}"},
            }]}]),
            "{role:?}"
        );
    }
}

// Source: roomkit-ai tests/chat_messages.rs::messages_keep_image_positions_and_errors_name_the_caller
#[test]
fn messages_keep_image_positions_and_errors_name_the_caller() {
    let image = AiContentPart::Image(AiImagePart {
        url: "data:;base64,YQ".to_owned(),
        mime_type: Some("image/png".to_owned()),
    });
    let message = AiMessage::new(
        AiRole::User,
        vec![
            AiContentPart::Text(AiTextPart::new("a")),
            image,
            AiContentPart::Text(AiTextPart::new("b")),
        ],
    );
    assert_eq!(
        Value::Array(chat_messages(&[message], None, None, "app").unwrap()),
        json!([{"role": "user", "content": [
            {"type": "text", "text": "a"},
            {"type": "image_url", "image_url": {"url": "data:image/png;base64,YQ=="}},
            {"type": "text", "text": "b"},
        ]}])
    );
    let bad = AiMessage::new(
        AiRole::User,
        vec![AiContentPart::Image(AiImagePart::new("data:broken"))],
    );
    let error = chat_messages(&[bad], None, None, "app").unwrap_err();
    assert_eq!(error.provider, "app");
    assert!(!error.retryable);
    assert_eq!(
        error.message,
        "invalid image part: data URI carries no payload"
    );
}

// Source: src/request.rs::tests::uri_is_rebuilt_for_a_data_uri_and_untouched_for_a_url
#[test]
fn uri_is_rebuilt_for_a_data_uri_and_untouched_for_a_url() {
    let sent = |url: &str| {
        let image = AiContentPart::Image(AiImagePart {
            url: url.to_owned(),
            mime_type: Some("image/png".to_owned()),
        });
        let rows = rendered(&[AiMessage::new(AiRole::User, vec![image])]);
        rows[0]["content"][0]["image_url"]["url"].clone()
    };
    assert_eq!(
        [
            sent("data:;base64,QUJD\nMTIz"),
            sent("data:image/png;name=lamp.png;base64,QUJDMTI"),
            sent("https://example.com/a.png"),
        ],
        [
            json!("data:image/png;base64,QUJDMTIz"),
            json!("data:image/png;base64,QUJDMTI="),
            json!("https://example.com/a.png"),
        ]
    );
}

// Source: roomkit-ai tests/chat_messages.rs::images_from_tool_results_follow_all_the_tool_rows
#[test]
fn images_from_tool_results_follow_all_the_tool_rows() {
    let content = ToolResult::Parts(vec![
        ToolResultPart::Text(AiTextPart::new("found")),
        ToolResultPart::Image(AiImagePart::new("https://example.org/image.png")),
    ]);
    let message = AiMessage::new(
        AiRole::Tool,
        vec![
            AiContentPart::ToolResult(AiToolResultPart::new("c2", "second", content)),
            AiContentPart::ToolResult(AiToolResultPart::new("c1", "first", "done")),
        ],
    );
    // An empty system prompt is none: no system message leads.
    assert_eq!(
        Value::Array(chat_messages(&[message], Some(""), None, "app").unwrap()),
        json!([
            {"role": "tool", "content": "found", "tool_call_id": "c2"},
            {"role": "tool", "content": "done", "tool_call_id": "c1"},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": "https://example.org/image.png"}},
            ]},
        ])
    );
}

/// What an assistant message that reasoned `thinking`, and called a tool
/// when `calls`, carries under `reasoning` where the configuration names
/// `named` for a model's earlier reasoning.
fn reasoning_field(named: Option<&str>, thinking: &str, calls: bool) -> Option<Value> {
    let mut parts = vec![AiContentPart::Thinking(AiThinkingPart::new(thinking))];
    if calls {
        parts.push(call_part());
    }
    let message = AiMessage::new(AiRole::Assistant, parts);
    let rows = chat_messages(&[message], None, named, "openai").unwrap();
    rows[0].get("reasoning").cloned()
}

// Source: src/dialect.rs::tests::a_named_field_carries_reasoning_and_leaves_a_round_that_did_not_reason_without_it
#[test]
fn a_named_field_carries_reasoning_and_leaves_a_round_that_did_not_reason_without_it() {
    let named = Some("reasoning");
    assert_eq!(reasoning_field(named, "why", true), Some(json!("why")));
    assert_eq!(reasoning_field(named, "why", false), Some(json!("why")));
    assert_eq!(reasoning_field(named, "", true), None);
    // No field named sends no reasoning.
    assert_eq!(reasoning_field(None, "why", true), None);
    assert_eq!(reasoning_field(None, "why", false), None);
}
