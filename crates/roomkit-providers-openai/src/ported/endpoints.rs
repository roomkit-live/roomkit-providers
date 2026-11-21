//! Where the provider sends its requests and what it decides from it: the
//! base URL it accepts, `OpenAI`'s own endpoint however it is written, and
//! what its `Debug` form shows.

use roomkit_providers_llm::{AiProvider, AiRequest, ProviderError};
use serde_json::{Value, json};

use super::support::{KEY, body_sent, hello, hello_with_a_tool, pointed, saying, tool};
use crate::replay::fake::Fake;
use crate::{OpenAi, OpenAiConfig};

/// The configuration calling `base_url` for `model`.
fn at(base_url: Option<&str>, model: &str) -> OpenAiConfig {
    OpenAiConfig {
        base_url: base_url.map(str::to_owned),
        ..OpenAiConfig::new(KEY, model)
    }
}

/// The effort a turn with tools sends, `base_url` the endpoint and `stated`
/// the switch, for a model the catalogue says keeps its effort beside
/// tools.
async fn tool_turn_effort(base_url: Option<&str>, stated: Option<bool>) -> Value {
    let config = OpenAiConfig {
        reasoning_effort: Some("low".to_owned()),
        supports_reasoning_effort_with_tools: stated,
        ..at(base_url, "gpt-5")
    };
    body_sent(config, &hello_with_a_tool()).await["reasoning_effort"].clone()
}

// Source: src/provider.rs::tests::on_openai_s_own_endpoint_a_tool_turn_keeps_its_effort
#[tokio::test]
async fn on_openai_s_own_endpoint_a_tool_turn_keeps_its_effort() {
    for base_url in [
        None,
        Some("https://api.openai.com/v1"),
        Some("https://api.openai.com/v1/"),
    ] {
        for stated in [None, Some(false)] {
            assert_eq!(
                tool_turn_effort(base_url, stated).await,
                json!("low"),
                "{base_url:?} {stated:?}"
            );
        }
    }
    // A base URL carrying credentials is another endpoint, which the switch
    // decides.
    let with_credentials = "https://user:placeholder-not-a-key@api.openai.com/v1";
    assert_eq!(
        tool_turn_effort(Some(with_credentials), None).await,
        Value::Null
    );
}

// Source: src/provider.rs::tests::the_endpoint_is_the_base_url_s_chat_completions
#[test]
fn the_endpoint_is_the_base_url_s_chat_completions() {
    let provider = OpenAi::new(at(Some("https://api.openai.com/v1/"), "gpt-4o")).unwrap();
    assert_eq!(
        provider.endpoint().as_str(),
        "https://api.openai.com/v1/chat/completions"
    );
}

/// Why no provider is built for `base_url`.
fn unbuilt(base_url: &str) -> ProviderError {
    OpenAi::new(at(Some(base_url), "gpt-4o")).unwrap_err()
}

// Source: src/provider.rs::tests::a_base_url_that_is_no_http_url_is_refused
#[test]
fn a_base_url_that_is_no_http_url_is_refused() {
    for base_url in ["api.openai.com/v1", "ftp://example.com", ""] {
        let error = unbuilt(base_url);
        assert!(
            error.message.contains("not an http or https URL"),
            "{base_url}: {error}"
        );
        assert!(!error.retryable, "{base_url}");
    }
}

// Source: src/provider.rs::tests::a_base_url_with_a_query_or_a_fragment_is_refused
#[test]
fn a_base_url_with_a_query_or_a_fragment_is_refused() {
    for base_url in [
        "https://llm.example/deployments/d?api-version=2024-10-21",
        "https://api.openai.com/v1#chat",
    ] {
        let error = unbuilt(base_url);
        assert!(
            error.message.contains("a query or a fragment"),
            "{base_url}: {error}"
        );
        assert!(!error.retryable, "{base_url}");
    }
}

// Source: src/provider.rs::tests::a_refused_base_url_is_not_repeated
#[test]
fn a_refused_base_url_is_not_repeated() {
    for base_url in [
        "ftp://user:placeholder-password@example.com/?token=placeholder-token",
        "ftp://user:placeholder-password@example.com/placeholder-path",
    ] {
        let error = unbuilt(base_url);
        let printed = format!("{error} {error:?}");
        assert!(!printed.contains("placeholder"), "{printed}");
    }
}

// Source: src/provider.rs::tests::the_provider_s_debug_form_hides_the_api_key_and_the_base_url_s_credentials
#[test]
fn the_provider_s_debug_form_hides_the_api_key_and_the_base_url_s_credentials() {
    let base_url = "https://user:placeholder-password@example.com:8443/v1";
    let provider = OpenAi::new(at(Some(base_url), "gpt-6-luna")).unwrap();
    let printed = format!("{provider:?}");
    for secret in [KEY, "user", "placeholder-password", "/v1"] {
        assert!(!printed.contains(secret), "{secret} in {printed}");
    }
}

/// Whether the provider calling `base_url` refuses a tool named `name`,
/// before it sends anything.
async fn refuses(base_url: &str, name: &str) -> bool {
    let fake = Fake::serving(vec![saying("ok")]).await;
    let provider = pointed(at(Some(base_url), "gpt-4o"), &fake);
    let request = AiRequest {
        tools: vec![tool(name)],
        ..hello()
    };
    let refused = match provider.generate(&request).await {
        Ok(_) => false,
        Err(error) => {
            assert!(error.before_request, "{base_url} {name}: {error}");
            true
        }
    };
    assert_eq!(fake.seen().is_empty(), refused, "{base_url} {name}");
    refused
}

// Source: src/names.rs::tests::a_tool_name_openai_refuses_is_refused_on_openai_s_endpoint_alone
#[tokio::test]
async fn a_tool_name_openai_refuses_is_refused_on_openai_s_endpoint_alone() {
    let openai = "https://api.openai.com/v1";
    let (long, longest) = ("d".repeat(129), "c".repeat(128));
    let with_credentials = "https://user:placeholder-not-a-key@api.openai.com/v1";
    assert_eq!(
        [
            refuses(openai, "files.read").await,
            refuses(openai, "files:read").await,
            refuses(openai, &long).await,
            refuses("HTTPS://API.OpenAI.com:443/v1/", "files.read").await,
            refuses(openai, &longest).await,
            refuses(openai, "files_read-2").await,
            refuses("http://localhost:8000/v1", "files.read").await,
            refuses(with_credentials, "files.read").await,
        ],
        [true, true, true, true, false, false, false, false]
    );
}
