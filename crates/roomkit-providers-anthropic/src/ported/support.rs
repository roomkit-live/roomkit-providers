//! What the tests share: the configurations they build, the answers the
//! fake gives, and a provider pointed at the fake.

use roomkit_providers_llm::{
    AiMessage, AiProvider, AiRequest, AiRole, AiTool, ProviderError, StreamEvent,
};
use serde_json::{Value, json};

use crate::replay::drained;
use crate::replay::fake::{Ending, Fake, Reply};
use crate::{Anthropic, AnthropicConfig};
pub(crate) use roomkit_providers_client::harness::text_of;

/// The key every test configures: a placeholder.
pub(crate) const KEY: &str = "placeholder-not-a-key";

/// A base URL that is not Anthropic's own.
const ELSEWHERE: &str = "https://llm.example";

/// The configuration calling Anthropic's own endpoint for `model`.
pub(crate) fn official(model: &str) -> AnthropicConfig {
    AnthropicConfig::new(KEY, model)
}

/// The configuration calling a server behind a base URL for `model`.
pub(crate) fn elsewhere(model: &str) -> AnthropicConfig {
    AnthropicConfig {
        base_url: Some(ELSEWHERE.to_owned()),
        ..AnthropicConfig::new(KEY, model)
    }
}

/// A request of one user message.
pub(crate) fn hello() -> AiRequest {
    AiRequest {
        messages: vec![AiMessage::new(AiRole::User, "Hi")],
        ..AiRequest::default()
    }
}

/// A tool named `name`.
pub(crate) fn tool(name: &str) -> AiTool {
    AiTool::new(name, "Looks something up").unwrap()
}

/// The provider of `config`, its requests sent to `fake`: what it knows of
/// its endpoint stays what `config` says.
pub(crate) fn pointed(config: AnthropicConfig, fake: &Fake) -> Anthropic {
    let mut provider = Anthropic::new(config).unwrap();
    provider.point_at(fake.address);
    provider
}

/// The body the provider of `config` writes for `request`.
pub(crate) fn body_of(config: AnthropicConfig, request: &AiRequest) -> Value {
    let provider = Anthropic::new(config).unwrap();
    Value::Object(provider.asked().body(request).unwrap())
}

/// One event of a stream, as Anthropic writes it: the line that names it,
/// then its data.
pub(crate) fn event(data: &Value) -> String {
    let kind = data["type"].as_str().unwrap_or_default();
    format!("event: {kind}\ndata: {data}\n\n")
}

/// The event that starts a message, which tells an input of 12 tokens.
pub(crate) fn start() -> Value {
    json!({"type": "message_start", "message": {
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "answering-model",
        "content": [],
        "usage": {"input_tokens": 12, "output_tokens": 1},
    }})
}

/// The events of a block of text at `index` that says `text`.
pub(crate) fn text_block(index: u32, text: &str) -> [Value; 3] {
    [
        json!({"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": index}),
    ]
}

/// The events that end a message for `stop_reason`, after 5 output tokens.
pub(crate) fn end(stop_reason: &str) -> [Value; 2] {
    [
        json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": {"output_tokens": 5}}),
        json!({"type": "message_stop"}),
    ]
}

/// The bytes of a stream that holds `events`, each as Anthropic writes it.
pub(crate) fn sse_of(events: &[Value]) -> Vec<u8> {
    events.iter().map(event).collect::<String>().into_bytes()
}

/// A streamed answer of `events`, sent in one read, after which the stream
/// ends.
pub(crate) fn stream_of(events: &[Value]) -> Reply {
    Reply::stream(vec![sse_of(events)], Ending::Ends)
}

/// The events of a whole message that says `text` and ends its turn.
pub(crate) fn message_saying(text: &str) -> Vec<Value> {
    let mut events = vec![start()];
    events.extend(text_block(0, text));
    events.extend(end("end_turn"));
    events
}

/// A streamed message that says `text`.
pub(crate) fn saying(text: &str) -> Reply {
    stream_of(&message_saying(text))
}

/// What `generate_structured_stream` gives of `request` where a server
/// behind a base URL answers with `reply`: its events, then the failure
/// that ended them.
pub(crate) async fn streamed(
    reply: Reply,
    request: &AiRequest,
) -> (Vec<StreamEvent>, Option<ProviderError>) {
    let fake = Fake::serving(vec![reply]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    drained(provider.generate_structured_stream(request)).await
}
