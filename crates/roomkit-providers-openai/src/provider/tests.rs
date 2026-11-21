//! What a provider refuses to be built with, and what its `Debug` form
//! says of where it sends.

use std::time::{Duration, Instant};

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
}

// Forms the client reads otherwise than they look, or otherwise than
// Python's client: the `Debug` form shows the client's reading, and the
// provider is not built.
#[test]
fn a_base_url_the_two_clients_read_otherwise_is_refused() {
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
        // User information that is no UTF-8 once decoded: Python's client
        // sends replaced characters, this one would send none or half.
        (
            "http://us%FFer:placeholder@llm.example/v1",
            "http://llm.example",
            "user information that is no UTF-8",
        ),
        (
            "http://user:pass%FFword@llm.example/v1",
            "http://llm.example",
            "user information that is no UTF-8",
        ),
        ("llm.example/v1", "(not a URL)", "not an http or https URL"),
        (
            "ftp://llm.example/v1",
            "ftp://llm.example",
            "not an http or https URL",
        ),
        ("https://", "(not a URL)", "not an http or https URL"),
    ];
    // A dot segment, written or encoded: the client takes it out of the
    // path, Python's sends it as it is.
    let dotted = [
        "https://llm.example/v1/..",
        "https://llm.example/v1/%2e%2e",
        "https://llm.example/v1/.%2E/x",
        "https://llm.example/./v1",
        "https://llm.example/v1/%2e/",
    ];
    let dotted = dotted.map(|base_url| (base_url, "https://llm.example", "a dot segment"));
    for (base_url, origin, why) in odd.into_iter().chain(dotted) {
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

// A timeout is refused while the clock could not count a hundred years past
// its end: the longest the clock takes now is not one a provider is built
// with, since the clock would fail on it a moment later.
#[test]
fn a_timeout_leaves_the_clock_a_hundred_years() {
    let counted = |seconds: u64| {
        let timeout = Duration::from_secs(seconds);
        Instant::now().checked_add(timeout).is_some()
    };
    // The longest timeout the clock counts now, found by halving.
    let (mut longest, mut too_long) = (0, u64::MAX);
    assert!(counted(longest) && !counted(too_long));
    while too_long - longest > 1 {
        let middle = longest + (too_long - longest) / 2;
        if counted(middle) {
            longest = middle;
        } else {
            too_long = middle;
        }
    }
    let years = |years: u64| years * 365 * 24 * 3600;
    let built = |seconds: u64, member: &str| {
        let timeout = Duration::from_secs(seconds);
        let mut config = behind("https://llm.example/v1");
        match member {
            "timeout" => config.timeout = timeout,
            _ => config.connect_timeout = timeout,
        }
        OpenAiAiProvider::new(config)
            .map(|_| ())
            .map_err(|error| error.message)
    };
    let refused = "the provider cannot be built: its timeout is longer than the clock can count";
    for member in ["timeout", "connect_timeout"] {
        for within in [0, 1, 50, 99] {
            let seconds = longest - years(within);
            assert_eq!(built(seconds, member), Err(refused.to_owned()), "{within}");
        }
        for below in [101, 200] {
            assert_eq!(built(longest - years(below), member), Ok(()), "{below}");
        }
    }
}

// A base URL is followed by the path of the API as Python's client follows
// it: one slash that ends it is the one before the path, and any other
// stays.
#[test]
fn the_path_of_the_api_follows_the_base_url_as_python_s_client_writes_it() {
    let paths = [
        ("https://llm.example/v1", "/v1/chat/completions"),
        ("https://llm.example/v1/", "/v1/chat/completions"),
        ("https://llm.example/v1//", "/v1//chat/completions"),
        ("https://llm.example", "/chat/completions"),
        ("https://llm.example/", "/chat/completions"),
        (
            "https://llm.example/a..b/.hidden/v1",
            "/a..b/.hidden/v1/chat/completions",
        ),
    ];
    for (base_url, path) in paths {
        let provider = OpenAiAiProvider::new(behind(base_url)).unwrap();
        assert_eq!(provider.endpoint().path(), path, "{base_url}");
    }
}
