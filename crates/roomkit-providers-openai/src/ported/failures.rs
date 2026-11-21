//! How a request that fails reads, and what a request never does: echo the
//! key, name its URL, follow a redirect, read an answer without bound.

use std::net::SocketAddr;

use roomkit_providers_llm::{AiProvider, ProviderError};
use tokio::net::TcpSocket;

use super::support::{KEY, asked, elsewhere, hello, json_reply, pointed, saying, streamed};
use crate::exchange::ANSWER_LIMIT;
use crate::replay::fake::{Body, Ending, Fake, Reply};
use crate::{OpenAi, OpenAiConfig};

/// More bytes than the provider reads of an answer.
const TOO_LONG: usize = ANSWER_LIMIT + 1024 * 1024;

fn read(error: &ProviderError) -> (bool, Option<u16>) {
    (error.retryable, error.status_code)
}

// Source: tests/agent/generate.rs::an_error_answer_past_the_answer_s_bound_keeps_its_status
// (the answer asked whole)
#[tokio::test]
async fn an_error_answer_past_the_answer_s_bound_keeps_its_status() {
    let body = vec![b'x'; TOO_LONG];
    let error = asked(json_reply(503, body.clone()), &hello()).await;
    assert_eq!(read(&error.unwrap_err()), (true, Some(503)));
    let (events, failure) = streamed(json_reply(503, body), &hello()).await;
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(read(&failure.unwrap()), (true, Some(503)));
}

// Source: tests/agent/main.rs::an_answer_declaring_more_than_the_limit_fails_the_turn
// and ::an_answer_streaming_past_the_limit_fails_the_turn (driven there
// through a room; here the answer is asked whole)
#[tokio::test]
async fn an_answer_past_the_limit_fails_the_request() {
    let json = || vec![("content-type".to_owned(), "application/json".to_owned())];
    let declaring = json_reply(200, vec![b' '; TOO_LONG]);
    let streaming = Reply::Answer {
        status: 200,
        headers: json(),
        body: Body::Reads(
            vec![vec![b' '; 64 * 1024]; TOO_LONG / (64 * 1024)],
            Ending::Ends,
        ),
    };
    for reply in [declaring, streaming] {
        let error = asked(reply, &hello()).await.unwrap_err();
        assert_eq!(read(&error), (false, None));
        assert_eq!(
            error.message,
            "The provider answered with a body it could not read: the provider's answer is \
             longer than 4194304 bytes"
        );
    }
}

// Source: tests/agent/main.rs::a_redirect_is_refused_and_the_request_never_follows_it
// (driven there through a room; here the answer is asked whole)
#[tokio::test]
async fn a_redirect_is_refused_and_the_request_never_follows_it() {
    let target = Fake::serving(vec![saying("Followed.")]).await;
    let location = format!("http://{}/v1/chat/completions", target.address);
    let redirect = Reply::Answer {
        status: 307,
        headers: vec![("location".to_owned(), location)],
        body: Body::Whole(Vec::new()),
    };
    let error = asked(redirect, &hello()).await.unwrap_err();
    assert_eq!(read(&error), (false, Some(307)));
    assert_eq!(
        error.message,
        "the endpoint answered with a redirect (307), which is not followed"
    );
    assert_eq!(target.seen().len(), 0, "nothing followed the redirect");
}

// Source: tests/agent/main.rs::the_api_key_an_error_answer_echoes_reaches_neither_the_error_nor_on_error
// (driven there through a room; here the answer is asked whole)
#[tokio::test]
async fn the_api_key_an_error_answer_echoes_reaches_no_failure() {
    let object = format!(
        r#"{{"error": {{"message": "Bearer {KEY} is refused", "type": "invalid_request_error", "code": "invalid_api_key"}}}}"#
    );
    let text = format!("Authorization: Bearer {KEY}\r\nrefused");
    // The error object's own words, the key removed; of a body that is no
    // JSON, nothing.
    let said = [
        "the provider answered HTTP 401 (invalid_request_error, invalid_api_key): \
         Bearer [***] is refused",
        "the provider answered HTTP 401: Unauthorized",
    ];
    for (body, said) in [object, text].into_iter().zip(said) {
        let error = asked(json_reply(401, body), &hello()).await.unwrap_err();
        assert_eq!(read(&error), (false, Some(401)));
        let printed = format!("{error} {error:?}");
        assert!(!printed.contains(KEY), "{printed}");
        assert_eq!(error.message, said);
    }
}

/// A local address whose connections are refused for as long as the socket
/// lives: bound, never listening. No other socket can bind it meanwhile.
fn refusing() -> (TcpSocket, SocketAddr) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind(([127, 0, 0, 1], 0).into()).unwrap();
    let address = socket.local_addr().unwrap();
    (socket, address)
}

// Source: tests/agent/main.rs::an_unreachable_endpoint_fails_the_turn_without_naming_its_url
// (driven there through a room; here the answer is asked whole)
#[tokio::test]
async fn an_unreachable_endpoint_fails_the_request_without_naming_its_url() {
    let (_refusing, address) = refusing();
    let config = OpenAiConfig {
        base_url: Some(format!("http://{address}/v1")),
        ..OpenAiConfig::new(KEY, "local-model")
    };
    let provider = OpenAi::new(config).unwrap();
    let error = provider.generate(&hello()).await.unwrap_err();
    assert_eq!(read(&error), (true, None));
    assert_eq!(error.message, "the connection to the provider failed");
    let printed = format!("{error} {error:?}");
    assert!(
        !printed.contains(&address.to_string()) && !printed.contains(&address.port().to_string()),
        "a base URL may carry a secret: {printed}"
    );
}

// Source: tests/agent/main.rs::an_empty_api_key_sends_no_authorization
// (driven there through a room; here the answer is asked whole)
#[tokio::test]
async fn an_empty_api_key_sends_no_authorization() {
    let fake = Fake::serving(vec![saying("Yes.")]).await;
    let config = OpenAiConfig {
        api_key: String::new(),
        ..elsewhere("local-model")
    };
    pointed(config, &fake).generate(&hello()).await.unwrap();
    assert_eq!(fake.seen()[0].headers.get("authorization"), None);
}

// Source: tests/agent/main.rs::a_request_asks_for_an_uncompressed_answer_even_with_gzip_enabled
// (driven there through a room; here the answer is asked whole)
#[tokio::test]
async fn a_request_asks_for_an_uncompressed_answer() {
    let fake = Fake::serving(vec![saying("Yes.")]).await;
    pointed(elsewhere("local-model"), &fake)
        .generate(&hello())
        .await
        .unwrap();
    assert_eq!(fake.seen()[0].headers.get("accept-encoding"), None);
}

// Source: tests/agent/retries.rs::a_try_again_carries_no_sdk_header
#[tokio::test]
async fn a_try_again_carries_no_sdk_header() {
    let soon = Reply::Answer {
        status: 503,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("retry-after-ms".to_owned(), "1".to_owned()),
        ],
        body: Body::Whole(b"{}".to_vec()),
    };
    let fake = Fake::serving(vec![soon, saying("Recovered")]).await;
    let config = OpenAiConfig {
        max_retries: 1,
        ..elsewhere("local-model")
    };
    let response = pointed(config, &fake).generate(&hello()).await.unwrap();
    assert_eq!(response.content, "Recovered");
    let requests = fake.seen();
    assert_eq!(requests.len(), 2);
    // The second try carries the headers of the first, and no other: none
    // that counts the tries, as the SDK's does.
    let [first, again] = requests.as_slice() else {
        panic!("two requests: {requests:?}");
    };
    assert_eq!(again.lines, first.lines);
    assert_eq!((again.method.as_str(), &again.body), ("POST", &first.body));
}
