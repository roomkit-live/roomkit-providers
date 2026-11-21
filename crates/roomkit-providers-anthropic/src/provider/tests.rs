//! What a provider refuses to be built with, what its `Debug` form says of
//! where it sends, and which key each of its requests carries.

use std::collections::BTreeMap;
use std::time::Duration;

use roomkit_providers_llm::{AiProvider, AiRequest};
use serde_json::json;

use crate::ported::support::{KEY, elsewhere, hello, official, pointed, saying};
use crate::replay::fake::{Fake, Reply};
use crate::{Anthropic, AnthropicConfig};

fn behind(base_url: &str) -> AnthropicConfig {
    AnthropicConfig {
        base_url: Some(base_url.to_owned()),
        ..AnthropicConfig::new(KEY, "local-model")
    }
}

fn with_headers(config: AnthropicConfig, headers: &[(&str, &str)]) -> AnthropicConfig {
    let headers = headers
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()));
    AnthropicConfig {
        extra_headers: Some(headers.collect::<BTreeMap<_, _>>()),
        ..config
    }
}

/// [`hello`], carrying `key` as the key of the request.
fn lending(key: &str) -> AiRequest {
    let mut request = hello();
    request.metadata.insert("api_key".to_owned(), key.into());
    request
}

/// The origin a `Debug` form shows for its base URL.
fn shown(config: &AnthropicConfig) -> String {
    let shown = format!("{config:?}");
    let (_, after) = shown.split_once("base_url: \"").unwrap();
    after.split_once('"').unwrap().0.to_owned()
}

// The `Debug` form names the origin the client dials, read as the client
// reads it: never another host than the one a request goes to.
// Source: src/tests.rs::the_catalogue_is_complete_and_configuration_debug_is_redacted
#[test]
fn the_debug_form_names_the_origin_the_client_dials_and_no_secret() {
    let dialled = [
        ("http://localhost:8080", "http://localhost:8080"),
        (
            "HTTPS://LLM.Example:8443/anthropic/",
            "https://llm.example:8443",
        ),
        (
            "https://user:placeholder@llm.example/private",
            "https://llm.example",
        ),
        ("http://[::1]:8000", "http://[::1]:8000"),
        ("https://llm.example:443/v1/messages", "https://llm.example"),
        ("http://127.1", "http://127.0.0.1"),
    ];
    for (base_url, origin) in dialled {
        let config = behind(base_url);
        assert_eq!(shown(&config), origin, "{base_url}");
        let provider = Anthropic::new(config).unwrap();
        let endpoint = provider.endpoint();
        let port = endpoint
            .port()
            .map_or_else(String::new, |port| format!(":{port}"));
        let host = endpoint.host_str().unwrap();
        assert_eq!(
            format!("{}://{host}{port}", endpoint.scheme()),
            origin,
            "{base_url}"
        );
        let shown = format!("{provider:?}");
        assert!(shown.contains(origin), "{shown}");
        for secret in ["placeholder", "private", "user"] {
            assert!(!shown.contains(secret), "{shown}");
        }
    }
    assert_eq!(
        shown(&official("claude-opus-5-5")),
        "https://api.anthropic.com"
    );
    assert!(Anthropic::available_models().len() >= 26);
}

// Source: src/tests.rs::a_pasted_messages_endpoint_is_normalized_without_dropping_a_bare_v1
// Python: tests/test_providers/test_anthropic.py::test_the_appended_path_is_dropped
#[test]
fn a_pasted_messages_endpoint_is_asked_once_and_a_bare_v1_is_kept() {
    for (base_url, expected) in [
        (
            " https://host/anthropic/v1/messages/ ",
            "https://host/anthropic/v1/messages",
        ),
        ("https://host/v1", "https://host/v1/v1/messages"),
        ("https://host/", "https://host/v1/messages"),
    ] {
        let provider = Anthropic::new(behind(base_url)).unwrap();
        assert_eq!(provider.endpoint().as_str(), expected, "{base_url:?}");
    }
    let provider = Anthropic::new(behind("https://host/anthropic")).unwrap();
    assert_eq!(
        provider.models.as_str(),
        "https://host/anthropic/v1/models?limit=1000"
    );
}

// A timeout the clock cannot count to the end of is refused when the
// provider is built: the HTTP client would fail on it at the first read.
#[tokio::test]
async fn a_timeout_the_clock_cannot_count_is_refused() {
    for member in ["timeout", "connect_timeout"] {
        let mut written = json!({"api_key": KEY, "model": "local-model"});
        written[member] = json!(1e19);
        let config: AnthropicConfig = serde_json::from_value(written).unwrap();
        let error = Anthropic::new(config).unwrap_err();
        assert_eq!(
            error.message,
            "the provider cannot be built: its timeout is longer than the clock can count"
        );
        assert!(error.before_request && !error.retryable);
        assert_eq!(error.provider, "anthropic");
    }
    // The longest timeout a provider is built with still reads an answer.
    let fake = Fake::serving(vec![saying("Yes."), saying("Yes.")]).await;
    for seconds in [3_000_000_000_u64, 5_000_000_000_000_000] {
        let config = AnthropicConfig {
            timeout: Duration::from_secs(seconds),
            connect_timeout: Duration::from_secs(seconds),
            ..behind("https://llm.example")
        };
        let provider = pointed(config, &fake);
        assert_eq!(provider.generate(&hello()).await.unwrap().content, "Yes.");
    }
}

// Source: src/tests/wire.rs::deferred_support_requires_an_official_endpoint_and_a_capable_model
// Python: src/roomkit/providers/anthropic/ai.py::AnthropicAIProvider.supports_deferred_tools
#[test]
fn deferred_support_requires_anthropic_s_endpoint_and_a_capable_model() {
    let holds = |config: AnthropicConfig| Anthropic::new(config).unwrap().supports_deferred_tools();
    assert!(holds(official("claude-sonnet-4-6")));
    assert!(!holds(official("claude-opus-4-1")));
    assert!(!holds(official("claude-future-unknown")));
    assert!(!holds(elsewhere("claude-sonnet-4-6")));
    assert!(holds(AnthropicConfig {
        base_url: Some("HTTPS://API.Anthropic.com:443/v1/messages/".to_owned()),
        ..official("claude-sonnet-4-6")
    }));
}

// Source: src/tests/wire.rs::extra_headers_replace_defaults_case_insensitively
// Python: src/roomkit/providers/anthropic/ai.py::AnthropicAIProvider._build_client,
// whose extra headers are the SDK's default headers and replace its own
#[tokio::test]
async fn extra_headers_replace_the_provider_s_own_whatever_their_case() {
    let extra = [
        ("X-API-Key", "placeholder-from-a-header"),
        ("Anthropic-Version", "custom"),
        ("X-Tenant", "acme"),
    ];
    let config = with_headers(elsewhere("local-model"), &extra);
    let fake = Fake::serving(vec![saying("Yes."), saying("Yes.")]).await;
    let provider = pointed(config, &fake);
    // The key an extra header carries goes in place of a request's too.
    for request in [hello(), lending("placeholder-lent-key")] {
        provider.generate(&request).await.unwrap();
    }
    for seen in fake.seen() {
        let sent = |name: &str| {
            let lines = seen.lines.iter().filter(|(sent, _)| sent == name);
            lines.map(|(_, value)| value.as_str()).collect::<Vec<_>>()
        };
        assert_eq!(sent("x-api-key"), ["placeholder-from-a-header"]);
        assert_eq!(sent("anthropic-version"), ["custom"]);
        assert_eq!(sent("x-tenant"), ["acme"]);
        assert_eq!(sent("accept"), ["application/json"]);
        assert_eq!(sent("content-type"), ["application/json"]);
    }
    // No `Debug` form shows the key a header carries.
    assert!(!format!("{provider:?}").contains("placeholder"));
}

// Python: tests/test_providers/test_anthropic.py::TestPerRequestCredential: each
// call is sent with the key its own request carries, and the
// configuration's when it carries none: no call leaves its key to the next.
// The listing is asked with the configuration's.
#[tokio::test]
async fn each_request_goes_with_its_own_key_and_leaves_none_to_the_next() {
    let replies = (0..5).map(|_| saying("Yes."));
    let listing = Reply::json(200, r#"{"data": []}"#);
    let fake = Fake::serving(replies.chain([listing]).collect()).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let mut no_text = hello();
    no_text.metadata.insert("api_key".to_owned(), 42.into());
    for request in [
        lending("placeholder-lent-key"),
        hello(),
        lending("placeholder-of-another"),
        lending(""),
        no_text,
    ] {
        provider.generate(&request).await.unwrap();
    }
    provider.list_models().await.unwrap();
    let keys: Vec<String> = fake
        .seen()
        .iter()
        .map(|seen| seen.headers["x-api-key"].clone())
        .collect();
    assert_eq!(
        keys,
        [
            "placeholder-lent-key",
            KEY,
            "placeholder-of-another",
            KEY,
            KEY,
            KEY
        ]
    );
}

// A request that has no key to carry is refused before it is sent, as
// Python's SDK refuses it: the configuration holds none, the request brings
// none, and no extra header carries a credential.
#[tokio::test]
async fn a_request_with_no_key_to_carry_is_not_sent() {
    let keyless = || AnthropicConfig {
        api_key: String::new(),
        ..elsewhere("local-model")
    };
    let fake = Fake::serving(vec![saying("Yes."), saying("Yes."), saying("Yes.")]).await;
    let provider = pointed(keyless(), &fake);
    let no_key = "the request carries no key: the configuration holds none, and neither does the \
                  request";
    for error in [
        provider.generate(&hello()).await.unwrap_err(),
        provider.list_models().await.unwrap_err(),
    ] {
        assert_eq!(error.message, no_key);
        assert_eq!((error.retryable, error.status_code), (false, None));
    }
    assert!(fake.seen().is_empty(), "nothing is sent");
    // The key of a request is one.
    provider
        .generate(&lending("placeholder-lent-key"))
        .await
        .unwrap();
    assert_eq!(fake.seen()[0].headers["x-api-key"], "placeholder-lent-key");
    // So is a credential among the extra headers: the request then carries
    // no key of the provider's.
    let bearer = [("Authorization", "Bearer placeholder-from-a-header")];
    let provider = pointed(with_headers(keyless(), &bearer), &fake);
    provider.generate(&hello()).await.unwrap();
    let seen = &fake.seen()[1];
    assert_eq!(
        seen.headers["authorization"],
        "Bearer placeholder-from-a-header"
    );
    assert!(!seen.headers.contains_key("x-api-key"));
    // A key that cannot go in a header is refused before anything is sent,
    // in words that repeat nothing of it.
    let error = pointed(elsewhere("local-model"), &fake)
        .generate(&lending("placeholder-with-a\nline-break"))
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        "the key of the request cannot go in a header"
    );
    assert!(error.before_request && !error.retryable);
    // An extra header that carries nothing is no credential.
    let empty = with_headers(keyless(), &[("X-Api-Key", ""), ("Authorization", "")]);
    let error = pointed(empty, &fake).generate(&hello()).await.unwrap_err();
    assert_eq!(error.message, no_key);
    assert_eq!(fake.seen().len(), 2);
}

// A base URL that holds user information is sent with it as Basic
// credentials, beside the key, as Python's client sends it: they take the
// place of a configured `Authorization`.
#[tokio::test]
async fn the_user_information_of_a_base_url_goes_as_basic_credentials_beside_the_key() {
    let config = with_headers(
        behind("https://user:word@llm.example"),
        &[("Authorization", "Bearer placeholder-from-a-header")],
    );
    let fake = Fake::serving(vec![saying("Yes.")]).await;
    pointed(config, &fake).generate(&hello()).await.unwrap();
    let seen = &fake.seen()[0];
    assert_eq!(seen.headers["authorization"], "Basic dXNlcjp3b3Jk");
    assert_eq!(seen.headers["x-api-key"], KEY);
    let authorizations = seen
        .lines
        .iter()
        .filter(|(name, _)| name == "authorization");
    assert_eq!(authorizations.count(), 1);
}
