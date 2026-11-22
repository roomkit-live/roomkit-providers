//! What a client reads of an answer asked whole: its body within the bound,
//! and what the server says it is; and what each request says of itself.

use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use roomkit_ai::ProviderError;

use crate::harness::fake::{Body, Ending, Fake, Reply};
use crate::testing::{client, left, url_on, usual};
use crate::{ANSWER_LIMIT, Call, Pace, is_json};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

/// What asking a fake that answers with `reply` gives.
async fn asked(reply: Reply) -> Result<(Vec<u8>, bool), ProviderError> {
    let fake = Fake::serving(vec![reply]).await;
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    let answered = client(usual(), 0).send(call).await;
    answered.map_err(|failure| left(&failure))
}

/// An answer of HTTP `status` that says nothing of what it is.
fn answer(status: u16, body: Body) -> Reply {
    Reply::Answer {
        status,
        headers: Vec::new(),
        body,
    }
}

// An answer is read whole up to 4 MiB, and a byte more fails the request,
// whether the server declares its length or sends it read by read. An
// error body past the bound keeps its status.
#[tokio::test]
async fn an_answer_is_read_whole_up_to_its_bound_and_no_further() {
    assert_eq!(ANSWER_LIMIT, 4 * 1024 * 1024);
    let declared = |length: usize| answer(200, Body::Whole(vec![b' '; length]));
    let in_reads = |length: usize| {
        let reads = vec![vec![b' '; length]];
        answer(200, Body::Reads(reads, Ending::Ends))
    };
    for reply in [declared(ANSWER_LIMIT), in_reads(ANSWER_LIMIT)] {
        let (body, json) = asked(reply).await.unwrap();
        assert_eq!((body.len(), json), (ANSWER_LIMIT, false));
    }
    let said = "The provider answered with a body it could not read: the provider's answer is \
                longer than 4194304 bytes";
    for reply in [declared(ANSWER_LIMIT + 1), in_reads(ANSWER_LIMIT + 1)] {
        let error = asked(reply).await.unwrap_err();
        assert_eq!(
            (error.message.as_str(), error.retryable, error.status_code),
            (said, false, None)
        );
    }
    let refused = answer(400, Body::Whole(vec![b' '; ANSWER_LIMIT + 1]));
    let error = asked(refused).await.unwrap_err();
    assert_eq!(
        (error.message.as_str(), error.status_code),
        ("the provider answered HTTP 400: Bad Request", Some(400))
    );
}

// An answer is JSON when its content type starts so, white space aside:
// not when it names JSON somewhere else, and not when it says nothing.
#[test]
fn an_answer_is_json_when_its_content_type_starts_so() {
    let says = |kind: &'static str| {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(kind));
        is_json(&headers)
    };
    for json in [
        "application/json",
        "application/json; charset=utf-8",
        " application/json",
    ] {
        assert!(says(json), "{json}");
    }
    for other in [
        "text/json",
        "text/event-stream",
        "application/x-ndjson",
        "text/plain; as=application/json",
    ] {
        assert!(!says(other), "{other}");
    }
    assert!(!is_json(&HeaderMap::new()));
}

// A request with a body says that its body is JSON, unless a header of its
// call says what it is. A listing has no body: it says nothing of one, and
// is asked with a GET.
#[tokio::test]
async fn a_request_says_what_its_body_is_unless_its_call_does() {
    let fake = Fake::serving(vec![Reply::json(200, "{}"); 3]).await;
    let client = client(usual(), 0);
    let (url, none) = (url_on(&fake, PATH), HeaderMap::new());
    let mut own = HeaderMap::new();
    let kind = HeaderValue::from_static("application/vnd.example+json");
    own.insert(CONTENT_TYPE, kind);
    let posted = Call {
        url: &url,
        headers: &none,
        body: Some(b"{}"),
        token: None,
    };
    client.send(posted).await.unwrap();
    let with_its_own = Call {
        headers: &own,
        ..posted
    };
    client.send(with_its_own).await.unwrap();
    let listing = Call {
        body: None,
        token: None,
        ..posted
    };
    client.send(listing).await.unwrap();
    let seen = fake.seen();
    let said: Vec<Vec<&str>> = seen
        .iter()
        .map(|seen| {
            let lines = seen.lines.iter();
            let kinds = lines.filter(|(name, _)| name == "content-type");
            kinds.map(|(_, kind)| kind.as_str()).collect()
        })
        .collect();
    let none: Vec<&str> = Vec::new();
    assert_eq!(
        said,
        [
            vec!["application/json"],
            vec!["application/vnd.example+json"],
            none
        ]
    );
    let methods: Vec<&str> = seen.iter().map(|seen| seen.method.as_str()).collect();
    assert_eq!(methods, ["POST", "POST", "GET"]);
}

// The clock a `retry-after` date is read against is the system's, in
// seconds since 1970, until a test replaces it.
#[test]
fn the_clock_of_a_client_is_the_system_s() {
    let now = (Pace::default().now)();
    let system = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    assert!((system.as_secs_f64() - now).abs() < 60.0, "{now}");
}
