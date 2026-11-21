//! What a provider refuses to be built with, and what its `Debug` form
//! says of where it sends.

use std::time::Duration;

use roomkit_providers_core::AiProvider;
use serde_json::json;

use crate::ported::support::{KEY, hello, pointed, saying};
use crate::replay::fake::Fake;
use crate::{OpenAiAiProvider, OpenAiConfig};

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
        let provider = OpenAiAiProvider::new(config).unwrap();
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
    // Forms the client reads otherwise than they look: the `Debug` form
    // shows the client's reading, and the provider is not built.
    let odd = [
        (
            "http://llm.example\\@evil.example/v1",
            "http://llm.example",
            "a backslash",
        ),
        (
            "http:///llm.example/v1",
            "http://llm.example",
            "names no host",
        ),
        (
            "http:llm.example/v1",
            "http://llm.example",
            "not an http or https URL",
        ),
        (
            " http://llm.example/v1 ",
            "http://llm.example",
            "white space",
        ),
        (
            "http://llm.example/v1\n",
            "http://llm.example",
            "white space",
        ),
        (
            "http://llm.exa\tmple/v1",
            "http://llm.example",
            "white space",
        ),
        (
            "http://llm.example/v 1",
            "http://llm.example",
            "white space",
        ),
        (
            "http://llm.example/v1\u{0}",
            "http://llm.example",
            "control character",
        ),
        ("llm.example/v1", "(not a URL)", "not an http or https URL"),
        (
            "ftp://llm.example/v1",
            "ftp://llm.example",
            "not an http or https URL",
        ),
        ("https://", "(not a URL)", "not an http or https URL"),
    ];
    for (base_url, origin, why) in odd {
        let config = behind(base_url);
        assert_eq!(shown(&config), origin, "{base_url:?}");
        let error = OpenAiAiProvider::new(config).unwrap_err();
        assert!(error.before_request && !error.retryable, "{base_url:?}");
        assert!(
            error
                .message
                .starts_with("the provider cannot be built: its base URL")
        );
        assert!(
            error.message.contains(why),
            "{base_url:?}: {}",
            error.message
        );
        assert!(!error.message.contains("example"), "{}", error.message);
    }
}

// A timeout the clock cannot count to the end of is refused when the
// provider is built: the HTTP client would fail on it at the first read.
#[tokio::test]
async fn a_timeout_the_clock_cannot_count_is_refused() {
    for member in ["timeout", "connect_timeout"] {
        let mut written = json!({"api_key": KEY, "model": "local-model"});
        written[member] = json!(1e19);
        let config: OpenAiConfig = serde_json::from_value(written).unwrap();
        let error = OpenAiAiProvider::new(config).unwrap_err();
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
