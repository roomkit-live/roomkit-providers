//! What a provider refuses to be built with, what its `Debug` form says of
//! where it sends, which key each of its requests carries, and how each
//! names its client.

use std::collections::BTreeMap;
use std::time::Duration;

use roomkit_ai::{AiContext, AiProvider};
use roomkit_providers_client::{Call, USER_AGENT};
use serde_json::json;

use crate::ported::support::{KEY, elsewhere, hello, official, pointed, saying};
use crate::replay::drained;
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
fn lending(key: &str) -> AiContext {
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
    // No `Debug` form shows the key a header carries: neither the
    // provider's, nor that of the headers a request goes with.
    assert!(!format!("{provider:?}").contains("placeholder"));
    for request in [hello(), lending("placeholder-lent-key")] {
        let headers = provider.headers_of(&request).unwrap();
        for (name, value) in [
            ("x-api-key", "placeholder-from-a-header"),
            ("anthropic-version", "custom"),
        ] {
            assert_eq!(headers.get_all(name).iter().count(), 1, "{name}");
            assert_eq!(headers[name], value);
        }
        let shown = format!("{headers:?}");
        assert!(!shown.contains("placeholder"), "{shown}");
    }
}

// A host may put a credential in an extra header of any name: the `Debug`
// form of what a request goes with, its headers and its call, shows each
// header's name and no configured value, for a request that goes with the
// configuration's key as for one that brings its own.
#[test]
fn the_debug_form_of_a_request_s_headers_shows_no_value_of_an_extra_header() {
    let extra = [
        ("X-Tenant", "placeholder-of-a-tenant"),
        ("Cookie", "placeholder-of-a-session"),
    ];
    let provider = Anthropic::new(with_headers(elsewhere("local-model"), &extra)).unwrap();
    for request in [hello(), lending("placeholder-lent-key")] {
        let headers = provider.headers_of(&request).unwrap();
        let call = Call {
            url: provider.endpoint(),
            headers: &headers,
            body: Some(b"{}"),
        };
        let shown = format!("{headers:?} {call:?}");
        assert!(
            shown.contains("x-tenant") && shown.contains("cookie"),
            "{shown}"
        );
        assert!(!shown.contains("placeholder"), "{shown}");
    }
}

// The value of an extra header is a secret of the exit, whatever the header:
// no failure repeats it, of a whole answer, of a stream or of the listing.
// Where the value names a scheme before its credentials, the credentials
// alone are one too.
#[tokio::test]
async fn no_failure_repeats_the_value_of_an_extra_header() {
    let extra = [
        ("X-Tenant", "placeholder-of-a-tenant"),
        ("Authorization", "Bearer placeholder-from-a-header"),
    ];
    let echoed = "Tenant placeholder-of-a-tenant may not send Bearer placeholder-from-a-header, \
                  nor placeholder-from-a-header.";
    let refusal = json!({"error": {"type": "permission_error", "message": echoed}}).to_string();
    let fake = Fake::serving(vec![Reply::json(403, refusal); 3]).await;
    let provider = pointed(with_headers(elsewhere("local-model"), &extra), &fake);
    let whole = provider.generate(&hello()).await.unwrap_err();
    let (_, streamed) = drained(provider.generate_structured_stream(&hello())).await;
    let listing = provider.list_models().await.unwrap_err();
    for error in [whole, streamed.unwrap(), listing] {
        assert_eq!(
            error.message,
            "the provider answered HTTP 403 (permission_error): Tenant [***] may not send [***], \
             nor [***]."
        );
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains("placeholder"), "{shown}");
    }
    // The headers went as they are configured.
    for seen in fake.seen() {
        assert_eq!(seen.headers["x-tenant"], "placeholder-of-a-tenant");
        assert_eq!(
            seen.headers["authorization"],
            "Bearer placeholder-from-a-header"
        );
    }
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

// Not in Python, whose HTTP client refuses such a value for a header and
// sends nothing: a key of the request that is blank, or written in white
// space, goes in its header as the request wrote it, and the server reads
// it without that white space. No failure repeats it, as it was lent or as
// the server read it.
#[tokio::test]
async fn a_key_of_the_request_in_white_space_goes_as_it_is_written() {
    let read = "placeholder-lent-key";
    let said = format!("Wrong key: {read}.");
    let refusal = json!({"error": {"type": "authentication_error", "message": said}});
    let replies = vec![Reply::json(401, refusal.to_string()), saying("Yes.")];
    let fake = Fake::serving(replies).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let lent = format!(" {read} ");
    let error = provider.generate(&lending(&lent)).await.unwrap_err();
    assert_eq!(
        error.message,
        "the provider answered HTTP 401 (authentication_error): Wrong key: [***]."
    );
    assert!(!format!("{error} {error:?}").contains(read));
    provider.generate(&lending("   ")).await.unwrap();
    let keys: Vec<String> = fake
        .seen()
        .iter()
        .map(|seen| seen.headers["x-api-key"].clone())
        .collect();
    assert_eq!(keys, [read, ""]);
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
    // So is a credential among the extra headers: the header of the key
    // then goes empty, once, as Python's SDK sends it.
    let bearer = [("Authorization", "Bearer placeholder-from-a-header")];
    let provider = pointed(with_headers(keyless(), &bearer), &fake);
    provider.generate(&hello()).await.unwrap();
    let seen = &fake.seen()[1];
    assert_eq!(
        seen.headers["authorization"],
        "Bearer placeholder-from-a-header"
    );
    let keys = seen.lines.iter().filter(|(name, _)| name == "x-api-key");
    assert_eq!(keys.map(|(_, key)| key.as_str()).collect::<Vec<_>>(), [""]);
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

// Every request names the client that sends it, `roomkit-providers/` and
// the version of the workspace, where Python's SDK names itself: an answer
// asked whole, a stream and the model listing. An extra header that names
// a `User-Agent`, in any case, replaces it.
#[tokio::test]
async fn every_request_names_its_client_unless_an_extra_header_does() {
    assert_eq!(
        USER_AGENT,
        format!("roomkit-providers/{}", env!("CARGO_PKG_VERSION"))
    );
    let replies = || {
        let listing = Reply::json(200, r#"{"data": []}"#);
        vec![saying("Yes."), saying("Yes."), listing]
    };
    let named = [("USER-agent", "example/1.0")];
    for (config, agent) in [
        (elsewhere("local-model"), USER_AGENT),
        (
            with_headers(elsewhere("local-model"), &named),
            "example/1.0",
        ),
    ] {
        let fake = Fake::serving(replies()).await;
        let provider = pointed(config, &fake);
        provider.generate(&hello()).await.unwrap();
        let (_, failure) = drained(provider.generate_structured_stream(&hello())).await;
        assert!(failure.is_none(), "{failure:?}");
        provider.list_models().await.unwrap();
        let seen = fake.seen();
        assert_eq!(seen.len(), 3);
        for request in seen {
            let lines = request.lines.iter();
            let agents = lines.filter(|(name, _)| name == "user-agent");
            let agents: Vec<_> = agents.map(|(_, value)| value.as_str()).collect();
            assert_eq!(agents, [agent], "{} {}", request.method, request.target);
        }
    }
}
