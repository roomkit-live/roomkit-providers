//! What the ported tests share: the configurations they build, the replies
//! the fake gives, and a provider pointed at the fake.

use std::error::Error as _;

use roomkit_providers_core::{
    AiMessage, AiProvider, AiRequest, AiResponse, AiRole, AiTool, ProviderError,
    ResponseSchemaError, ResponseSchemaFailure,
};
use serde_json::{Value, json};

use crate::replay::fake::{Body, Fake, Reply};
use crate::{OpenAiAiProvider, OpenAiConfig};

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

/// [`hello`], declaring one tool.
pub(crate) fn hello_with_a_tool() -> AiRequest {
    AiRequest {
        tools: vec![tool("lookup")],
        ..hello()
    }
}

/// An answer of HTTP `status` whose JSON body is `body`.
pub(crate) fn json_reply(status: u16, body: impl Into<Vec<u8>>) -> Reply {
    Reply::Answer {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Body::Whole(body.into()),
    }
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
pub(crate) fn pointed(config: OpenAiConfig, fake: &Fake) -> OpenAiAiProvider {
    let mut provider = OpenAiAiProvider::new(config).unwrap();
    let dialed = provider.endpoint().clone();
    let target = &dialed[url::Position::BeforePath..];
    provider.point_at(&format!("http://{}{target}", fake.address));
    provider
}

/// The JSON body of the request `fake` received at `at`.
pub(crate) fn body_of(fake: &Fake, at: usize) -> Value {
    serde_json::from_slice(&fake.seen()[at].body).unwrap()
}

/// The body the provider of `config` sends for `request`.
pub(crate) async fn body_sent(config: OpenAiConfig, request: &AiRequest) -> Value {
    let fake = Fake::serving(vec![saying("ok")]).await;
    pointed(config, &fake).generate(request).await.unwrap();
    body_of(&fake, 0)
}

/// The failure of `request`, which the provider of `config` refuses before
/// it sends anything.
pub(crate) async fn refused(config: OpenAiConfig, request: &AiRequest) -> ProviderError {
    let fake = Fake::serving(vec![saying("never")]).await;
    let error = pointed(config, &fake).generate(request).await.unwrap_err();
    assert!(fake.seen().is_empty(), "nothing is sent");
    error
}

/// What `generate` gives of `request` where a server behind a base URL
/// answers with `reply`.
pub(crate) async fn asked(reply: Reply, request: &AiRequest) -> Result<AiResponse, ProviderError> {
    let fake = Fake::serving(vec![reply]).await;
    pointed(elsewhere("local-model"), &fake)
        .generate(request)
        .await
}

/// Why `error` says a response schema failed, when it is one's failure.
pub(crate) fn schema_failure(error: &ProviderError) -> Option<ResponseSchemaFailure> {
    let failure = error.source()?.downcast_ref::<ResponseSchemaError>()?;
    Some(failure.reason)
}
