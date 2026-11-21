//! What a provider refuses to be built with, what its `Debug` form says of
//! where it sends, and how each of its requests names its client.

use std::time::Duration;

use roomkit_providers_client::USER_AGENT;
use roomkit_providers_llm::AiProvider;
use serde_json::json;

use crate::ported::support::{KEY, chunk, elsewhere, ending, events, hello, pointed, saying};
use crate::replay::drained;
use crate::replay::fake::{Fake, Reply};
use crate::{OpenAi, OpenAiConfig};

fn behind(base_url: &str) -> OpenAiConfig {
    OpenAiConfig {
        base_url: Some(base_url.to_owned()),
        ..OpenAiConfig::new(KEY, "local-model")
    }
}

/// The origin a `Debug` form shows for its base URL.
fn shown(config: &OpenAiConfig) -> String {
    let shown = format!("{config:?}");
    let (_, after) = shown.split_once("base_url: \"").unwrap();
    after.split_once('"').unwrap().0.to_owned()
}

// The `Debug` form names the origin the client dials, read as the client
// reads it: never another host than the one a request goes to.
#[test]
fn the_debug_form_names_the_origin_the_client_dials() {
    let dialled = [
        ("http://localhost:11434/v1", "http://localhost:11434"),
        ("HTTPS://LLM.Example:8443/v1/", "https://llm.example:8443"),
        (
            "https://user:placeholder@llm.example/v1",
            "https://llm.example",
        ),
        ("http://[::1]:8000/v1", "http://[::1]:8000"),
        ("https://llm.example:443/v1", "https://llm.example"),
        ("http://127.1/v1", "http://127.0.0.1"),
    ];
    for (base_url, origin) in dialled {
        let config = behind(base_url);
        assert_eq!(shown(&config), origin, "{base_url}");
        let provider = OpenAi::new(config).unwrap();
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
        assert!(format!("{provider:?}").contains(origin), "{provider:?}");
    }
}

// A host may put a credential in a default header of any name: the `Debug`
// form of what a request goes with, its headers and its call, shows each
// header's name and no configured value.
#[test]
fn the_debug_form_of_a_request_s_headers_shows_no_value_of_a_default_header() {
    let headers = [
        ("X-Tenant", "placeholder-of-a-tenant"),
        ("Cookie", "placeholder-of-a-session"),
    ];
    let headers = headers.map(|(name, value)| (name.to_owned(), value.to_owned()));
    let config = OpenAiConfig {
        default_headers: Some(headers.into_iter().collect()),
        ..elsewhere("local-model")
    };
    let provider = OpenAi::new(config).unwrap();
    let shown = format!("{:?} {:?}", provider.headers, provider.asking(b"{}"));
    assert!(
        shown.contains("x-tenant") && shown.contains("cookie"),
        "{shown}"
    );
    assert!(!shown.contains("placeholder"), "{shown}");
}

// A timeout the clock cannot count to the end of is refused when the
// provider is built: the HTTP client would fail on it at the first read.
#[tokio::test]
async fn a_timeout_the_clock_cannot_count_is_refused() {
    for member in ["timeout", "connect_timeout"] {
        let mut written = json!({"api_key": KEY, "model": "local-model"});
        written[member] = json!(1e19);
        let config: OpenAiConfig = serde_json::from_value(written).unwrap();
        let error = OpenAi::new(config).unwrap_err();
        assert_eq!(
            error.message,
            "the provider cannot be built: its timeout is longer than the clock can count"
        );
        assert!(error.before_request && !error.retryable);
    }
    // The longest timeout a provider is built with still reads an answer.
    let fake = Fake::serving(vec![saying("Yes."), saying("Yes.")]).await;
    for seconds in [3_000_000_000_u64, 5_000_000_000_000_000] {
        let config = OpenAiConfig {
            timeout: Duration::from_secs(seconds),
            connect_timeout: Duration::from_secs(seconds),
            ..behind("https://llm.example/v1")
        };
        let provider = pointed(config, &fake);
        assert_eq!(provider.generate(&hello()).await.unwrap().content, "Yes.");
    }
}

// Every request names the client that sends it, `roomkit-providers/` and
// the version of the workspace, where Python's SDK names itself: an answer
// asked whole, a stream and the model listing. A default header that names
// a `User-Agent`, in any case, replaces it.
#[tokio::test]
async fn every_request_names_its_client_unless_a_default_header_does() {
    assert_eq!(
        USER_AGENT,
        format!("roomkit-providers/{}", env!("CARGO_PKG_VERSION"))
    );
    let replies = || {
        let mut chunks = vec![chunk(&json!({"content": "Yes."}))];
        chunks.extend(ending("stop", 12, 3));
        let listing = Reply::json(200, r#"{"data": []}"#);
        vec![saying("Yes."), events(&chunks), listing]
    };
    let header = ("USER-agent".to_owned(), "example/1.0".to_owned());
    let named = OpenAiConfig {
        default_headers: Some([header].into_iter().collect()),
        ..elsewhere("local-model")
    };
    for (config, agent) in [
        (elsewhere("local-model"), USER_AGENT),
        (named, "example/1.0"),
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
