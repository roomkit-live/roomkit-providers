//! What a base URL is refused for when a provider is built, how the path
//! of an API follows one, what is shown of one, and which base URLs are a
//! vendor's own endpoint.

use roomkit_providers_llm::ProviderError;

use super::*;
use crate::failure::{Exit, Secrets};

/// A path of an API, under a base URL.
const PATH: &str = "/chat/completions";

/// Why no provider is built for `base_url`, as the failure reads once it
/// left a provider with no secret.
fn unbuilt(base_url: &str) -> ProviderError {
    let failure = joined(base_url, PATH).unwrap_err();
    Exit::new("test", Secrets::default()).leave(&failure)
}

#[test]
fn a_url_is_shown_without_its_user_information_path_or_query() {
    let shown = |url: &str| shown_url(url);
    assert_eq!(shown("https://api.openai.com/v1"), "https://api.openai.com");
    assert_eq!(
        shown(
            "HTTPS://user:placeholder-not-a-key@LLM.Example:8443/v1/placeholder?key=placeholder#f"
        ),
        "https://llm.example:8443"
    );
    assert_eq!(shown("http://[::1]:8000/v1"), "http://[::1]:8000");
    assert_eq!(shown("http://localhost:11434/v1"), "http://localhost:11434");
    for hidden in [
        "placeholder-not-a-key",
        "llm.example/v1",
        "https://",
        "https://llm.example:port/v1",
        "https://user@placeholder not a key/v1",
        "https://[placeholder]/v1",
    ] {
        assert_eq!(shown(hidden), "(not a URL)", "{hidden}");
    }
}

// Forms the client reads otherwise than they look, or otherwise than
// Python's client: what is shown of each is the client's reading, and no
// provider is built for it.
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
        assert_eq!(shown_url(base_url), origin, "{base_url:?}");
        let error = unbuilt(base_url);
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
        let url = joined(base_url, PATH).unwrap();
        assert_eq!(url.path(), path, "{base_url}");
    }
}

// Python: is_vendor_endpoint(base_url, *official): none given, or one of
// the vendor's official URLs however it is written; a URL that carries
// credentials, another port or another path is another endpoint.
#[test]
fn a_base_url_is_the_vendor_s_own_under_any_of_its_official_urls() {
    let official = [
        "https://api.vendor.example/v1",
        "https://api.vendor.example",
    ];
    let own = |base_url: &str| is_vendor_endpoint(Some(base_url), &official);
    assert!(is_vendor_endpoint(None, &official));
    for base_url in [
        "https://api.vendor.example/v1",
        "https://api.vendor.example",
        "HTTPS://API.Vendor.Example:443/v1//",
        " https://api.vendor.example/ ",
        "https://@api.vendor.example/v1",
    ] {
        assert!(own(base_url), "{base_url:?}");
    }
    for base_url in [
        "http://api.vendor.example/v1",
        "https://api.vendor.example:8443/v1",
        "https://api.vendor.example:port/v1",
        "https://api.vendor.example/v2",
        "https://user:placeholder@api.vendor.example/v1",
        "https://[api.vendor.example]/v1",
        "https://llm.example/v1",
        "api.vendor.example/v1",
    ] {
        assert!(!own(base_url), "{base_url:?}");
    }
    // A scheme's default port is none, whichever the scheme.
    let plain = ["http://localhost/v1"];
    assert!(is_vendor_endpoint(Some("http://localhost:80/v1"), &plain));
    assert!(!is_vendor_endpoint(Some("http://localhost:443/v1"), &plain));
}

// A provider talks to its vendor's own endpoint when its base URL is the
// vendor's and the client dials the vendor's host for it.
#[test]
fn a_provider_talks_to_its_vendor_where_the_client_dials_the_vendor_s_url() {
    let official = ["https://api.vendor.example/v1"];
    let talks = |base_url: Option<&str>| {
        let dialed = joined(base_url.unwrap_or("https://api.vendor.example/v1"), PATH).unwrap();
        talks_to_vendor(base_url, &official, &dialed, PATH)
    };
    assert!(talks(None));
    assert!(talks(Some("HTTPS://API.Vendor.Example:443/v1/")));
    assert!(!talks(Some("https://llm.example/v1")));
    assert!(!talks(Some(
        "https://user:placeholder@api.vendor.example/v1"
    )));
    // The vendor's base URL, dialed elsewhere, is not the vendor's endpoint.
    let elsewhere = joined("https://llm.example/v1", PATH).unwrap();
    assert!(!talks_to_vendor(None, &official, &elsewhere, PATH));
}

// A base URL with a query, and one with a fragment, are each refused: the
// path of the API can follow neither.
#[test]
fn a_base_url_with_a_query_or_a_fragment_is_refused() {
    let said = "the provider cannot be built: its base URL has a query or a fragment, which the \
                path of the API cannot follow";
    for base_url in [
        "https://llm.example/v1?version=1",
        "https://llm.example/v1#part",
        "https://llm.example/v1?",
    ] {
        assert_eq!(unbuilt(base_url).message, said, "{base_url}");
    }
}

// A URL holds user information when it names a user, a password, or both.
#[test]
fn a_url_holds_user_information_with_a_user_or_with_a_password() {
    let holds = |url: &str| has_user_information(&reqwest::Url::parse(url).unwrap());
    assert!(holds("https://user@llm.example/v1"));
    assert!(holds("https://:placeholder@llm.example/v1"));
    assert!(holds("https://user:placeholder@llm.example/v1"));
    assert!(!holds("https://llm.example/v1"));
}

// Python: is_vendor_endpoint, on what `urlsplit` reads otherwise than it
// looks: an empty pair before the host is no credentials, a tab is no part
// of a URL, a port that is no number names no endpoint, and what starts
// with a digit is no scheme, so it is compared with nothing.
#[test]
fn a_base_url_is_compared_as_python_splits_it() {
    const OFFICIAL: &str = "https://api.vendor.example/v1";
    let is_vendor_s = |url: &str| is_vendor_endpoint(Some(url), &[OFFICIAL]);
    assert!(is_vendor_s("https://:@api.vendor.example/v1"));
    assert!(is_vendor_s("https://api.vendor\t.example/v1"));
    assert!(!is_vendor_s("https://api.vendor.example:+443/v1"));
    let numbered = "9https://api.vendor.example/v1";
    assert!(!is_vendor_endpoint(Some(numbered), &[numbered]));
}

// A provider talks to its vendor only where the client dials the vendor's
// own URL, by its scheme, its host, its port and its path, with no user
// information; and only under a base URL that is the vendor's.
#[test]
fn a_provider_talks_to_its_vendor_at_the_vendor_s_own_url_alone() {
    const OFFICIAL: &str = "https://api.vendor.example/v1";
    let talks = |base_url: Option<&str>, dialed: &str| {
        let dialed = reqwest::Url::parse(dialed).unwrap();
        talks_to_vendor(base_url, &[OFFICIAL], &dialed, PATH)
    };
    let own = format!("{OFFICIAL}{PATH}");
    assert!(talks(None, &own));
    for elsewhere in [
        "http://api.vendor.example:443/v1/chat/completions",
        "https://api.vendor.example:8443/v1/chat/completions",
        "https://api.vendor.example/v2/chat/completions",
        "https://user@api.vendor.example/v1/chat/completions",
        "https://:placeholder@api.vendor.example/v1/chat/completions",
    ] {
        assert!(!talks(None, elsewhere), "{elsewhere}");
    }
    assert!(!talks(Some("https://llm.example/v1"), &own));
}
