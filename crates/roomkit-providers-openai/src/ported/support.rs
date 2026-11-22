//! What the ported tests share: the configurations they build, the replies
//! the fake gives, and a provider pointed at the fake.

use std::error::Error as _;

use roomkit_ai::{
    AiContext, AiMessage, AiProvider, AiResponse, AiRole, AiTool, ProviderError,
    ResponseSchemaError, ResponseSchemaFailure, StreamEvent,
};
use serde_json::{Value, json};

use crate::replay::drained;
use crate::replay::fake::{Ending, Fake, Reply};
use crate::{OpenAi, OpenAiConfig};
pub(crate) use roomkit_providers_client::harness::text_of;

/// The key every test configures: a placeholder.
pub(crate) const KEY: &str = "placeholder-not-a-key";

/// A base URL that is not `OpenAI`'s own.
const ELSEWHERE: &str = "https://llm.example/v1";

/// The configuration calling `OpenAI`'s own endpoint for `model`.
pub(crate) fn official(model: &str) -> OpenAiConfig {
    OpenAiConfig::new(KEY, model)
}

/// The configuration calling a server behind a base URL for `model`.
pub(crate) fn elsewhere(model: &str) -> OpenAiConfig {
    OpenAiConfig {
        base_url: Some(ELSEWHERE.to_owned()),
        ..OpenAiConfig::new(KEY, model)
    }
}

/// A request of one user message.
pub(crate) fn hello() -> AiContext {
    AiContext {
        messages: vec![AiMessage::new(AiRole::User, "Hi")],
        ..AiContext::default()
    }
}

/// A tool named `name`.
pub(crate) fn tool(name: &str) -> AiTool {
    AiTool::new(name, "Looks something up").unwrap()
}

/// [`hello`], declaring one tool.
pub(crate) fn hello_with_a_tool() -> AiContext {
    AiContext {
        tools: vec![tool("lookup")],
        ..hello()
    }
}

/// An answer of HTTP `status` whose JSON body is `body`.
pub(crate) fn json_reply(status: u16, body: impl Into<Vec<u8>>) -> Reply {
    Reply::json(status, body)
}

/// A completion whose one choice holds `message` and ends for
/// `finish_reason`.
pub(crate) fn completion(message: &Value, finish_reason: &str) -> Reply {
    let body = json!({
        "id": "chatcmpl-1",
        "object": "chat.completion",
        "model": "answering-model",
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15},
    });
    json_reply(200, body.to_string())
}

/// A completion that says `text`.
pub(crate) fn saying(text: &str) -> Reply {
    completion(&json!({"role": "assistant", "content": text}), "stop")
}

/// The provider of `config`, its requests sent to `fake`: what it knows of
/// its endpoint stays what `config` says.
pub(crate) fn pointed(config: OpenAiConfig, fake: &Fake) -> OpenAi {
    let mut provider = OpenAi::new(config).unwrap();
    provider.point_at(fake.address);
    provider
}

/// The JSON body of the request `fake` received at `at`.
pub(crate) fn body_of(fake: &Fake, at: usize) -> Value {
    serde_json::from_slice(&fake.seen()[at].body).unwrap()
}

/// The body the provider of `config` sends for `request`.
pub(crate) async fn body_sent(config: OpenAiConfig, request: &AiContext) -> Value {
    let fake = Fake::serving(vec![saying("ok")]).await;
    pointed(config, &fake).generate(request).await.unwrap();
    body_of(&fake, 0)
}

/// The failure of `request`, which the provider of `config` refuses before
/// it sends anything.
pub(crate) async fn refused(config: OpenAiConfig, request: &AiContext) -> ProviderError {
    let fake = Fake::serving(vec![saying("never")]).await;
    let error = pointed(config, &fake).generate(request).await.unwrap_err();
    assert!(fake.seen().is_empty(), "nothing is sent");
    error
}

/// What `generate` gives of `request` where a server behind a base URL
/// answers with `reply`.
pub(crate) async fn asked(reply: Reply, request: &AiContext) -> Result<AiResponse, ProviderError> {
    let fake = Fake::serving(vec![reply]).await;
    pointed(elsewhere("local-model"), &fake)
        .generate(request)
        .await
}

/// A streamed answer of HTTP 200: `reads`, each sent as one read, and how
/// they end.
pub(crate) fn stream_reply(reads: Vec<Vec<u8>>, ending: Ending) -> Reply {
    Reply::stream(reads, ending)
}

/// The text of a stream of events: one event for each of `chunks`, then
/// the end mark.
pub(crate) fn sse_of(chunks: &[Value]) -> String {
    let events = chunks.iter().map(|chunk| format!("data: {chunk}\n\n"));
    events.chain(["data: [DONE]\n\n".to_owned()]).collect()
}

/// A streamed completion: one event for each of `chunks`, then the end
/// mark, sent in one read.
pub(crate) fn events(chunks: &[Value]) -> Reply {
    stream_reply(vec![sse_of(chunks).into_bytes()], Ending::Ends)
}

/// One chunk of a streamed completion, its first choice's `delta`.
pub(crate) fn chunk(delta: &Value) -> Value {
    json!({"object": "chat.completion.chunk", "choices": [{"index": 0, "delta": delta}]})
}

/// The last chunks of a streamed completion: its end, then its usage.
pub(crate) fn ending(finish_reason: &str, prompt: u64, completion: u64) -> [Value; 2] {
    [
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}]}),
        json!({"choices": [], "usage": {"prompt_tokens": prompt, "completion_tokens": completion}}),
    ]
}

/// What `generate_structured_stream` gives of `request` where a server
/// behind a base URL answers with `reply`: its events, then the failure
/// that ended them.
pub(crate) async fn streamed(
    reply: Reply,
    request: &AiContext,
) -> (Vec<StreamEvent>, Option<ProviderError>) {
    let fake = Fake::serving(vec![reply]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    drained(provider.generate_structured_stream(request)).await
}

/// Why `error` says a response schema failed, when it is one's failure.
pub(crate) fn schema_failure(error: &ProviderError) -> Option<ResponseSchemaFailure> {
    let failure = error.source()?.downcast_ref::<ResponseSchemaError>()?;
    Some(failure.reason)
}
