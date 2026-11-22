//! What a request carries: its token bound, the switches of its
//! configuration, and its conversation as Chat Completions messages.

use roomkit_ai::{
    AiContentPart, AiContext, AiImagePart, AiMessage, AiRole, AiTextPart, AiThinkingPart,
    AiToolCallPart,
};
use serde_json::{Value, json};

use super::support::{KEY, body_sent, elsewhere, hello};
use crate::OpenAiConfig;
use crate::dialect::OPENAI_CHAT;

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
        let request = AiContext {
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

/// `messages` as `OpenAI`'s endpoint reads them, no field named for a
/// model's earlier reasoning.
fn rendered(messages: &[AiMessage]) -> Value {
    let request = AiContext {
        messages: messages.to_vec(),
        ..AiContext::default()
    };
    Value::Array(OPENAI_CHAT.messages(&request, "openai").unwrap())
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
