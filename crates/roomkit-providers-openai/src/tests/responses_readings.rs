//! What the Responses API carries and reads beside the room's loop: a
//! response schema, a model's reasoning, a failed stream, the effort and an
//! author's image. Python's provider has no Responses API, so no case
//! captured on Python holds these: each comes from the crate this provider
//! was written from, named by its file there.

use roomkit_ai::{
    AiContentPart, AiContext, AiImagePart, AiMessage, AiProvider, AiRole, AiTextPart, AiTool,
    ProviderError, ResponseSchemaFailure, StreamEvent,
};
use roomkit_providers_client::items::next;
use serde_json::{Map, Value, json};

use super::support::{
    KEY, body_of, body_sent, elsewhere, events, hello, hello_with_a_tool, json_reply, official,
    pointed, schema_failure, text_of,
};
use crate::replay::drained;
use crate::replay::fake::{Fake, Reply};
use crate::{OpenAi, OpenAiApi, OpenAiConfig};

/// Python's `_VERDICT`.
fn verdict() -> Map<String, Value> {
    let Value::Object(schema) = json!({
        "type": "object",
        "properties": { "label": { "type": "string" } },
        "required": ["label"],
        "additionalProperties": false,
    }) else {
        unreachable!("an object")
    };
    schema
}

/// A question under the verdict's schema, with `tools`.
fn asked(tools: Vec<AiTool>) -> AiContext {
    AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Is it?")],
        response_schema: Some(verdict()),
        tools,
        ..AiContext::default()
    }
}

fn lookup() -> AiTool {
    AiTool::new("lookup", "Look it up").unwrap()
}

/// A Responses stream ending `status`, its message saying `parts` (each an
/// `output_text` or a `refusal`), cut for `incomplete` when it gives why.
fn responded(parts: &[(&str, &str)], status: &str, incomplete: Option<&str>) -> Reply {
    let content: Vec<Value> = parts
        .iter()
        .map(|(kind, said)| match *kind {
            "refusal" => json!({ "type": "refusal", "refusal": said }),
            _ => json!({ "type": "output_text", "text": said, "annotations": [] }),
        })
        .collect();
    let mut response = json!({
        "id": "resp_1", "status": status, "model": "fake-model",
        "output": [{ "type": "message", "role": "assistant", "content": content }],
    });
    if let Some(reason) = incomplete {
        response["incomplete_details"] = json!({ "reason": reason });
    }
    events(&[json!({ "type": format!("response.{status}"), "response": response })])
}

/// The provider on the Responses API, behind a base URL, against a fake
/// answering `replies`.
async fn on_responses(replies: Vec<Reply>) -> (OpenAi, Fake) {
    let fake = Fake::serving(replies).await;
    let config = OpenAiConfig {
        api: OpenAiApi::Responses,
        ..elsewhere("fake-model")
    };
    (pointed(config, &fake), fake)
}

// roomkit-rs: tests/agent/response_schema.rs. The Responses API carries a
// response schema as Chat does: strictly, in its `text.format`, the answer
// checked before its end.
#[tokio::test]
async fn the_responses_api_carries_the_schema_as_its_text_format() {
    let replies = vec![responded(
        &[("text", r#"{"label": "yes"}"#)],
        "completed",
        None,
    )];
    let (provider, fake) = on_responses(replies).await;
    let response = provider.generate(&asked(Vec::new())).await.unwrap();
    assert_eq!(response.content, r#"{"label": "yes"}"#);
    assert_eq!(
        body_of(&fake, 0)["text"],
        json!({ "format": {
            "type": "json_schema", "name": "response", "schema": verdict(), "strict": true } })
    );
    let context = AiContext {
        response_schema: None,
        ..asked(Vec::new())
    };
    let (provider, fake) =
        on_responses(vec![responded(&[("text", "Hi")], "completed", None)]).await;
    provider.generate(&context).await.unwrap();
    assert_eq!(body_of(&fake, 0).get("text"), None);
}

// roomkit-rs: tests/agent/response_schema.rs. The Responses API fails an
// answer without its document as Chat does: a refusal part, an answer cut
// for its output cap or by a content filter, prose, streamed or whole.
#[tokio::test]
async fn a_responses_answer_without_its_document_fails_as_on_chat() {
    let cases = [
        (
            responded(&[("refusal", "I can't help with that.")], "completed", None),
            ResponseSchemaFailure::Refusal,
        ),
        (
            responded(
                &[("text", r#"{"label": "y"#)],
                "incomplete",
                Some("max_output_tokens"),
            ),
            ResponseSchemaFailure::Truncated,
        ),
        (
            responded(&[("text", "")], "incomplete", Some("content_filter")),
            ResponseSchemaFailure::Refusal,
        ),
        (
            responded(&[("text", "Yes, it is.")], "completed", None),
            ResponseSchemaFailure::InvalidJson,
        ),
    ];
    for (reply, expected) in cases {
        let (provider, _fake) = on_responses(vec![reply.clone(), reply]).await;
        let error = provider.generate(&asked(Vec::new())).await.unwrap_err();
        assert_eq!(schema_failure(&error), Some(expected), "{error}");
        let context = asked(Vec::new());
        let (events, error) = drained(provider.generate_structured_stream(&context)).await;
        assert_eq!(schema_failure(&error.unwrap()), Some(expected));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, StreamEvent::Done(_))),
            "the failure replaces the end"
        );
    }
}

// roomkit-rs: tests/agent/response_schema.rs. Without a schema, a refusal on
// the Responses API stays the answer's text, and its end names it, as on
// Chat.
#[tokio::test]
async fn a_responses_refusal_without_a_schema_is_text_its_end_naming_it() {
    let reply = responded(&[("refusal", "Cannot help.")], "completed", None);
    let (provider, _fake) = on_responses(vec![reply]).await;
    let context = AiContext {
        response_schema: None,
        ..asked(Vec::new())
    };
    let (events, error) = drained(provider.generate_structured_stream(&context)).await;
    assert!(error.is_none(), "{error:?}");
    let refusal = events.iter().find_map(|event| match event {
        StreamEvent::Done(done) => done.metadata.get("refusal").cloned(),
        StreamEvent::TextDelta(_)
        | StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::ThinkingDelta(_)
        | StreamEvent::Usage(_) => None,
    });
    assert_eq!(
        (text_of(&events), refusal),
        ("Cannot help.".to_owned(), Some(json!("Cannot help.")))
    );
}

// roomkit-rs: tests/agent/response_schema.rs. The Responses API takes Chat's
// defaults: a schema by default whatever the URL, beside tools only on
// OpenAI's own endpoint, the configuration deciding when it says.
#[tokio::test]
async fn the_responses_api_takes_chat_s_defaults() {
    let read = |stated: Option<bool>, with_tools: Option<bool>| {
        let provider = OpenAi::new(OpenAiConfig {
            api: OpenAiApi::Responses,
            base_url: Some("http://127.0.0.1:9/v1".to_owned()),
            supports_response_schema: stated,
            supports_response_schema_with_tools: with_tools,
            ..OpenAiConfig::new(KEY, "fake-model")
        })
        .unwrap();
        (
            provider.supports_response_schema(),
            provider.supports_response_schema_with_tools(),
        )
    };
    let official = OpenAi::new(OpenAiConfig {
        api: OpenAiApi::Responses,
        ..OpenAiConfig::new(KEY, "gpt-6-luna")
    })
    .unwrap();
    assert_eq!(
        [
            read(None, None),
            read(Some(false), None),
            read(None, Some(true)),
            (
                official.supports_response_schema(),
                official.supports_response_schema_with_tools()
            ),
        ],
        [(true, false), (false, false), (true, true), (true, true)]
    );
    // Beside tools behind a base URL, refused before any request.
    let (provider, fake) =
        on_responses(vec![responded(&[("text", "{}")], "completed", None)]).await;
    let context = asked(vec![lookup()]);
    let (_, error) = drained(provider.generate_structured_stream(&context)).await;
    assert_eq!(
        schema_failure(&error.unwrap()),
        Some(ResponseSchemaFailure::Unsupported)
    );
    assert_eq!(fake.seen().len(), 0, "no request is sent");
}

/// A piece of a stream, as the provider hands it.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Thinking(String),
    Text(String),
}

fn thinking(text: &str) -> Piece {
    Piece::Thinking(text.to_owned())
}

fn text(text: &str) -> Piece {
    Piece::Text(text.to_owned())
}

/// The reasoning and text `reply` streams on the Responses API, in order.
async fn streamed(reply: Reply) -> Vec<Piece> {
    let (provider, _fake) = on_responses(vec![reply]).await;
    let context = hello();
    let mut events = provider.generate_structured_stream(&context);
    let mut pieces = Vec::new();
    while let Some(event) = next(&mut events).await {
        match event.unwrap() {
            StreamEvent::ThinkingDelta(delta) => pieces.push(Piece::Thinking(delta.thinking)),
            StreamEvent::TextDelta(delta) => pieces.push(Piece::Text(delta.text)),
            StreamEvent::ToolCall(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::Usage(_) => {}
        }
    }
    pieces
}

fn reasoning(summary: &str) -> Value {
    json!({ "type": "reasoning", "id": "rs_1", "encrypted_content": "opaque",
        "summary": [{ "type": "summary_text", "text": summary }] })
}

fn said(text: &str) -> Value {
    json!({ "type": "message", "role": "assistant", "status": "completed",
        "content": [{ "type": "output_text", "text": text, "annotations": [] }] })
}

fn terminal(output: &[Value]) -> Value {
    json!({ "type": "response.completed", "response": {
        "id": "resp_1", "status": "completed", "model": "fake", "output": output,
        "usage": { "input_tokens": 1, "output_tokens": 1 } } })
}

// roomkit-rs: tests/agent/thinking.rs. A Responses reasoning item's summary
// is the model's reasoning, read whole or streamed.
#[tokio::test]
async fn a_responses_reasoning_item_reads_as_thinking() {
    let done = terminal(&[reasoning("Weighing."), said("42.")]);
    let read = streamed(events(std::slice::from_ref(&done))).await;
    assert_eq!(read, [thinking("Weighing."), text("42.")]);
    let whole = json_reply(200, done["response"].to_string());
    let (provider, _fake) = on_responses(vec![whole]).await;
    let response = provider.generate(&hello()).await.unwrap();
    assert_eq!(response.thinking.as_deref(), Some("Weighing."));
    assert_eq!(response.content, "42.");
}

// roomkit-rs: tests/agent/thinking.rs. Reasoning deltas come as written, and
// the terminal response repeats none.
#[tokio::test]
async fn a_responses_stream_s_reasoning_deltas_come_once() {
    let sent = [
        json!({ "type": "response.reasoning_summary_text.delta", "delta": "Weigh" }),
        json!({ "type": "response.reasoning_summary_text.delta", "delta": "ing." }),
        json!({ "type": "response.output_text.delta", "delta": "42." }),
        terminal(&[reasoning("Weighing."), said("42.")]),
    ];
    let read = streamed(events(&sent)).await;
    assert_eq!(read, [thinking("Weigh"), thinking("ing."), text("42.")]);
}

// roomkit-rs: tests/agent/thinking.rs. A reasoning delta that is no text or
// empty reads as nothing, the terminal response's reasoning then standing.
#[tokio::test]
async fn a_responses_reasoning_delta_with_nothing_in_it_reads_as_none() {
    for delta in [json!(""), json!(null)] {
        let sent = [
            json!({ "type": "response.reasoning_summary_text.delta", "delta": delta }),
            terminal(&[reasoning("Weighing."), said("42.")]),
        ];
        let read = streamed(events(&sent)).await;
        assert_eq!(read, [thinking("Weighing."), text("42.")]);
    }
}

/// The failure a Responses stream answering `reply` ends with.
async fn failure(reply: Reply) -> ProviderError {
    let (provider, _fake) = on_responses(vec![reply]).await;
    let context = hello();
    let (_, error) = drained(provider.generate_structured_stream(&context)).await;
    error.expect("the stream ended without a failure")
}

// roomkit-rs: tests/agent/generate.rs. A Responses stream that fails reads
// as the status its error's code names, as every provider reads one (RFC
// §6.7): `server_error` is a retried 500, a code no table names stays
// final.
#[tokio::test]
async fn a_failed_responses_stream_reads_as_the_status_its_code_names() {
    let failed = |code: &str| {
        events(&[
            json!({ "type": "response.failed", "response": { "status": "failed",
            "error": { "code": code, "message": "The server had an error." } } }),
        ])
    };
    let (server, quota) = (
        failure(failed("server_error")).await,
        failure(failed("quota_exceeded")).await,
    );
    let read = |error: &ProviderError| (error.retryable, error.status_code);
    assert_eq!(
        (read(&server), read(&quota)),
        ((true, Some(500)), (false, None))
    );
}

/// The effort the request for `model` carries on the Responses API, on
/// `OpenAI`'s own endpoint: `configured` the configuration's, `turn` the
/// turn's own, with a tool or without.
async fn effort_sent(
    model: &str,
    configured: Option<&str>,
    turn: Option<&str>,
    tools: bool,
) -> Value {
    let config = OpenAiConfig {
        api: OpenAiApi::Responses,
        reasoning_effort: configured.map(str::to_owned),
        ..official(model)
    };
    let mut asked = if tools { hello_with_a_tool() } else { hello() };
    asked.reasoning_effort = turn.map(str::to_owned);
    let fake = Fake::serving(vec![responded(&[("text", "ok")], "completed", None)]).await;
    pointed(config, &fake).generate(&asked).await.unwrap();
    body_of(&fake, 0)["reasoning"]["effort"].clone()
}

// roomkit-rs: src/reasoning/tests.rs, read here on the wire. On the
// Responses API the turn's effort stands over the configured one and an
// effort of `none` goes as the model's floor, but the tool-turn locks,
// measured on Chat Completions, do not apply.
#[tokio::test]
async fn on_the_responses_api_the_floor_applies_and_the_tool_turn_locks_do_not() {
    assert_eq!(
        effort_sent("gpt-5", Some("none"), None, false).await,
        json!("minimal")
    );
    assert_eq!(
        effort_sent("gpt-6-luna", Some("high"), None, true).await,
        json!("high")
    );
    assert_eq!(
        effort_sent("gpt-5-mini", Some("high"), Some("low"), true).await,
        json!("low")
    );
    assert_eq!(effort_sent("gpt-5", None, None, false).await, Value::Null);
}

// roomkit-rs: src/wire.rs, read here on the wire. An author's image goes as
// each API's own part, under the same URL.
#[tokio::test]
async fn author_images_use_native_parts_in_both_protocols() {
    let context = AiContext {
        messages: vec![AiMessage::new(
            AiRole::User,
            vec![
                AiContentPart::Text(AiTextPart::new("Describe this image")),
                AiContentPart::Image(AiImagePart::new("data:image/png;base64,AQIDBA==")),
            ],
        )],
        ..AiContext::default()
    };
    let config = |api| OpenAiConfig {
        api,
        vision: true,
        ..elsewhere("vision-fixture")
    };
    let chat = body_sent(config(OpenAiApi::ChatCompletions), &context).await;
    assert_eq!(
        chat.pointer("/messages/0/content/0/text"),
        Some(&json!("Describe this image"))
    );
    assert_eq!(
        chat.pointer("/messages/0/content/1/type"),
        Some(&json!("image_url"))
    );
    let fake = Fake::serving(vec![responded(&[("text", "ok")], "completed", None)]).await;
    let provider = pointed(config(OpenAiApi::Responses), &fake);
    provider.generate(&context).await.unwrap();
    let response = body_of(&fake, 0);
    assert_eq!(
        response.pointer("/input/0/content/1/type"),
        Some(&json!("input_image"))
    );
    assert_eq!(
        response.pointer("/input/0/content/1/image_url"),
        chat.pointer("/messages/0/content/1/image_url/url")
    );
}
