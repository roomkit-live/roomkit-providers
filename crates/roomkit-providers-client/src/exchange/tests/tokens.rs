//! The token of a host's source: asked before each try, sent as the try's
//! bearer, and shown by nothing the client hands on of the answer.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use crate::failure::{Failure, MESSAGE_LIMIT, told};
use crate::harness::fake::{Ending, Fake, Reply};
use crate::proxy::Proxies;
use crate::testing::{left, no_authority, settings, url_on, usual};
use crate::{Call, Client, Pace, TokenError, TokenFuture, TokenSource};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

/// A source whose token changes each time it is asked.
struct Rotating(AtomicUsize);

impl TokenSource for Rotating {
    fn token(&self) -> TokenFuture<'_> {
        let asked = self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move { Ok(format!("rotating-{asked}")) })
    }
}

/// A source that gives no token.
struct Refused;

impl TokenSource for Refused {
    fn token(&self) -> TokenFuture<'_> {
        Box::pin(async { Err(TokenError::SignInRequired) })
    }
}

/// A client that tries a failed request `max_retries` times again at once,
/// and says of a refusal what its body says.
fn telling_the_body(max_retries: u32) -> Client {
    let mut settings = settings(usual(), max_retries);
    settings.refused = |status, body| {
        let said = String::from_utf8_lossy(body).into_owned();
        Failure::said(vec![told(said, MESSAGE_LIMIT)], false, Some(status))
    };
    let mut client = Client::built(settings, Proxies::default(), no_authority()).unwrap();
    client.set_pace(Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(|_| Box::pin(std::future::ready(()))),
    });
    client
}

/// The `Authorization` of each request `fake` saw, in order.
fn authorizations(fake: &Fake) -> Vec<Option<String>> {
    let seen = fake.seen().into_iter();
    seen.map(|seen| seen.headers.get("authorization").cloned())
        .collect()
}

// The source is asked again for each request, its token is the request's
// bearer in place of the one its headers carry, and what a refusal's body
// says of the token it was sent with is not handed on.
#[tokio::test]
async fn a_source_is_asked_for_each_request_and_a_refusal_shows_no_token() {
    let refusal = r#"{"error":{"message":"credential rotating-1 refused"}}"#;
    let fake = Fake::serving(vec![Reply::json(200, "{}"), Reply::json(401, refusal)]).await;
    let client = telling_the_body(0);
    let source = Rotating(AtomicUsize::new(0));
    let url = url_on(&fake, PATH);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer unused-static"),
    );
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&source),
    };
    client.send(call).await.unwrap();
    let failed = left(&client.send(call).await.unwrap_err());
    assert!(!failed.message.contains("rotating-1"), "{}", failed.message);
    assert!(
        failed.message.contains("credential [***] refused"),
        "{}",
        failed.message
    );
    let sent =
        [Some("Bearer rotating-0"), Some("Bearer rotating-1")].map(|sent| sent.map(str::to_owned));
    assert_eq!(authorizations(&fake), sent);
    assert_eq!(source.0.load(Ordering::SeqCst), 2);
}

// A request tried again asks its source again, as Python's SDK refreshes a
// key before each try.
#[tokio::test]
async fn each_try_of_a_request_asks_its_source_again() {
    let unavailable = Reply::json(503, "{}");
    let fake = Fake::serving(vec![unavailable, Reply::json(200, "{}")]).await;
    let client = telling_the_body(1);
    let source = Rotating(AtomicUsize::new(0));
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&source),
    };
    client.send(call).await.unwrap();
    let sent =
        [Some("Bearer rotating-0"), Some("Bearer rotating-1")].map(|sent| sent.map(str::to_owned));
    assert_eq!(authorizations(&fake), sent);
}

// A source that gives no token ends the request before it is sent, in the
// source's own words: the bearer of the request's headers is not sent in
// its place.
#[tokio::test]
async fn a_source_that_gives_no_token_ends_the_request_and_nothing_is_sent() {
    let fake = Fake::serving(vec![Reply::json(200, "{}")]).await;
    let client = telling_the_body(1);
    let url = url_on(&fake, PATH);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer unused-static"),
    );
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&Refused),
    };
    let failed = left(&client.send(call).await.unwrap_err());
    assert!(
        failed.message.contains("sign in again"),
        "{}",
        failed.message
    );
    assert!(failed.before_request && !failed.retryable, "{failed:?}");
    assert!(fake.seen().is_empty());
}

// A whole answer shows no token either: read whole, and as the JSON
// document a stream was answered with.
#[tokio::test]
async fn a_whole_answer_shows_no_token() {
    let saying = |token: &str| format!(r#"{{"detail":"credential {token} refused"}}"#);
    let replies = ["rotating-0", "rotating-1"].map(|token| Reply::json(200, saying(token)));
    let fake = Fake::serving(replies.into()).await;
    let client = telling_the_body(0);
    let source = Rotating(AtomicUsize::new(0));
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&source),
    };
    let (whole, _) = client.send(call).await.unwrap();
    assert_eq!(String::from_utf8(whole).unwrap(), saying("[***]"));
    let mut answer = client.stream(call).await.unwrap();
    assert_eq!(answer.next().await.unwrap(), None);
    let unstreamed = answer.unstreamed().unwrap();
    assert_eq!(unstreamed.to_string(), saying("[***]"));
}

/// A source whose token is empty.
struct Empty;

impl TokenSource for Empty {
    fn token(&self) -> TokenFuture<'_> {
        Box::pin(async { Ok(String::new()) })
    }
}

// An empty token leaves its try without an `Authorization`: the bearer of
// the request's headers is not sent in its place.
#[tokio::test]
async fn an_empty_token_sends_no_authorization() {
    let fake = Fake::serving(vec![Reply::json(200, "{}")]).await;
    let client = telling_the_body(0);
    let url = url_on(&fake, PATH);
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_static("Bearer unused-static"),
    );
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&Empty),
    };
    client.send(call).await.unwrap();
    assert_eq!(authorizations(&fake), [None]);
}

// What the events of a stream say of the token it was asked with is not
// handed on either, nor what a JSON body in place of a stream says.
#[tokio::test]
async fn the_events_of_a_stream_show_no_token() {
    let event = b"data: {\"error\":\"credential rotating-0 refused\"}\n\n".to_vec();
    let fake = Fake::serving(vec![Reply::stream(vec![event], Ending::Ends)]).await;
    let client = telling_the_body(0);
    let source = Rotating(AtomicUsize::new(0));
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: Some(&source),
    };
    let mut answer = client.stream(call).await.unwrap();
    let data = answer.next().await.unwrap().unwrap();
    assert_eq!(data, r#"{"error":"credential [***] refused"}"#);
    assert_eq!(
        authorizations(&fake),
        [Some("Bearer rotating-0".to_owned())]
    );
}
