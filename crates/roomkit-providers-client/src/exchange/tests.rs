//! The times a request is given, and what a request that never left reads
//! as. No test here waits on a timeout: the time is let pass at once, after
//! what must have happened first is seen to have happened.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT as USER_AGENT_HEADER};

use super::{Call, Pace, USER_AGENT, configured_header};
use crate::Timeouts;
use crate::harness::fake::{Ending, Fake, Reply};
use crate::harness::{let_pass, poll_once};
use crate::testing::{client, left, url_on, usual};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

// A request that gets no answer fails once the client's timeout passed, and
// not before: half of it later the request still waits, and once it passed
// the failure comes long before another half.
#[tokio::test]
async fn a_request_that_gets_no_answer_fails_at_the_read_timeout() {
    const TIMEOUT: Duration = Duration::from_secs(40);
    let fake = Fake::serving(vec![Reply::Silent]).await;
    let timeouts = Timeouts::new(TIMEOUT, usual().connect()).unwrap();
    let client = client(timeouts, 0);
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    let mut asked = std::pin::pin!(client.send(call));
    tokio::select! {
        biased;
        answered = &mut asked => panic!("no answer comes: {answered:?}"),
        () = fake.silent() => {}
    }
    let_pass(TIMEOUT / 2).await;
    assert!(poll_once(&mut asked).await.is_pending());
    let_pass(TIMEOUT / 2 + Duration::from_secs(1)).await;
    let failed = tokio::select! {
        biased;
        failed = &mut asked => failed,
        () = tokio::time::sleep(TIMEOUT / 2) => panic!("the request outlives its timeout"),
    };
    let error = left(&failed.unwrap_err());
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the provider did not answer in time", true, None)
    );
}

// A connection that does not open fails at the client's connect timeout,
// long before the timeout of a read. The address is a local one whose
// listener accepts nothing and whose queue of connections is full, where
// Linux leaves a new connection waiting.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn a_connection_that_does_not_open_fails_at_the_connect_timeout() {
    const CONNECT: Duration = Duration::from_secs(5);
    const READ: Duration = Duration::from_secs(600);
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    socket.bind(([127, 0, 0, 1], 0).into()).unwrap();
    let listener = socket.listen(0).unwrap();
    let address = listener.local_addr().unwrap();
    let connect = || tokio::spawn(tokio::net::TcpStream::connect(address));
    let filling: Vec<_> = (0..4).map(|_| connect()).collect();
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    let client = client(Timeouts::new(READ, CONNECT).unwrap(), 0);
    let url = reqwest::Url::parse(&format!("http://{address}{PATH}")).unwrap();
    let headers = HeaderMap::new();
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    let mut asked = std::pin::pin!(client.send(call));
    // The connection begins, and waits.
    for _ in 0..16 {
        assert!(poll_once(&mut asked).await.is_pending());
        tokio::task::yield_now().await;
    }
    let_pass(CONNECT + Duration::from_secs(1)).await;
    let failed = tokio::select! {
        biased;
        failed = &mut asked => failed,
        () = tokio::time::sleep(READ / 20) => panic!("the connection outlives its timeout"),
    };
    let error = left(&failed.unwrap_err());
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the provider did not answer in time", true, None)
    );
    drop((listener, filling));
}

// Every request names the client that sends it, once: one that asks an
// answer whole, one that asks a stream and one that asks a listing. A
// header of the provider's that names another replaces it.
#[tokio::test]
async fn every_request_names_the_client_unless_a_header_of_the_provider_s_does() {
    assert_eq!(
        USER_AGENT,
        format!("roomkit-providers/{}", env!("CARGO_PKG_VERSION"))
    );
    let whole = || Reply::json(200, "{}");
    let stream = Reply::stream(vec![b"data: one\n\n".to_vec()], Ending::Ends);
    let fake = Fake::serving(vec![whole(), stream, whole(), whole()]).await;
    let client = client(usual(), 0);
    let (url, own) = (url_on(&fake, PATH), HeaderMap::new());
    let asked = Call {
        url: &url,
        headers: &own,
        body: Some(b"{}"),
    };
    client.send(asked).await.unwrap();
    let mut streamed = client.stream(asked).await.unwrap();
    while streamed.next().await.unwrap().is_some() {}
    let listing = Call {
        body: None,
        ..asked
    };
    client.send(listing).await.unwrap();
    let mut named = HeaderMap::new();
    named.insert(USER_AGENT_HEADER, HeaderValue::from_static("example/1.0"));
    let renamed = Call {
        headers: &named,
        ..asked
    };
    client.send(renamed).await.unwrap();
    let seen = fake.seen();
    let agents = seen.iter().map(|seen| {
        let lines = seen.lines.iter().filter(|(name, _)| name == "user-agent");
        lines.map(|(_, value)| value.as_str()).collect::<Vec<_>>()
    });
    assert_eq!(
        agents.collect::<Vec<_>>(),
        [[USER_AGENT], [USER_AGENT], [USER_AGENT], ["example/1.0"]]
    );
}

// The user information of a URL goes as Basic credentials, as with Python's
// client: they are the one `Authorization` of the request, in place of the
// one its call holds, for an answer asked whole, a stream and a listing. A
// URL that holds none leaves the call its own.
#[tokio::test]
async fn the_user_information_of_a_url_takes_the_place_of_a_call_s_authorization() {
    const OWN: &str = "Bearer placeholder-not-a-key";
    const BASIC: &str = "Basic dXNlcjpwbGFjZWhvbGRlci1ub3QtYS1wYXNzd29yZA==";
    let whole = || Reply::json(200, "{}");
    let stream = Reply::stream(vec![b"data: one\n\n".to_vec()], Ending::Ends);
    let fake = Fake::serving(vec![whole(), stream, whole(), whole()]).await;
    let client = client(usual(), 0);
    let with_a_user = format!(
        "http://user:placeholder-not-a-password@{}{PATH}",
        fake.address
    );
    let (with_a_user, bare) = (
        reqwest::Url::parse(&with_a_user).unwrap(),
        url_on(&fake, PATH),
    );
    let mut own = HeaderMap::new();
    own.insert(AUTHORIZATION, HeaderValue::from_static(OWN));
    let asked = Call {
        url: &with_a_user,
        headers: &own,
        body: Some(b"{}"),
    };
    client.send(asked).await.unwrap();
    let mut streamed = client.stream(asked).await.unwrap();
    while streamed.next().await.unwrap().is_some() {}
    let listing = Call {
        body: None,
        ..asked
    };
    client.send(listing).await.unwrap();
    let without = Call {
        url: &bare,
        ..asked
    };
    client.send(without).await.unwrap();
    let seen = fake.seen();
    let authorizations = seen.iter().map(|seen| {
        let lines = seen.lines.iter();
        let lines = lines.filter(|(name, _)| name == "authorization");
        lines.map(|(_, value)| value.as_str()).collect::<Vec<_>>()
    });
    assert_eq!(
        authorizations.collect::<Vec<_>>(),
        [[BASIC], [BASIC], [BASIC], [OWN]]
    );
}

// The `Debug` form of a call says where it goes by its origin, which
// headers it carries and how many bytes its body is: nothing of what may
// hold a secret, the rest of its URL, the value of a header, the body.
#[test]
fn the_debug_form_of_a_call_shows_no_user_information_header_value_or_body() {
    let url = "https://user:placeholder-not-a-password@llm.example:8443/v1/hidden?key=hidden";
    let url = reqwest::Url::parse(url).unwrap();
    let mut headers = HeaderMap::new();
    let value = HeaderValue::from_static("placeholder-not-a-value");
    headers.insert("x-tenant", value);
    let body = br#"{"said":"hidden"}"#;
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(body),
    };
    let shown = format!("{call:?}");
    let expected =
        r#"Call { url: "https://llm.example:8443", headers: ["x-tenant"], body_bytes: Some(17) }"#;
    assert_eq!(shown, expected);
    for hidden in ["user", "placeholder", "hidden", &format!("{body:?}")] {
        assert!(!shown.contains(hidden), "{hidden} in {shown}");
    }
    let listing = Call { body: None, ..call };
    assert!(format!("{listing:?}").ends_with("body_bytes: None }"));
}

// A host may put a credential in a header of any name: the value of a
// header of the configuration is shown by no `Debug` form, its own, that of
// the headers it is among or that of a call. What is no HTTP header is
// none.
#[test]
fn the_value_of_a_configured_header_is_shown_by_no_debug_form() {
    let (name, value) = configured_header("X-Tenant", "placeholder-not-a-value").unwrap();
    assert_eq!(
        (name.as_str(), value.to_str().unwrap()),
        ("x-tenant", "placeholder-not-a-value")
    );
    let mut headers = HeaderMap::new();
    headers.insert(name, value.clone());
    let url = reqwest::Url::parse("https://llm.example/v1").unwrap();
    let call = Call {
        url: &url,
        headers: &headers,
        body: None,
    };
    let shown = format!("{value:?} {headers:?} {call:?}");
    assert!(shown.contains("x-tenant"), "{shown}");
    assert!(!shown.contains("placeholder"), "{shown}");
    for (name, value) in [
        ("a name", "value"),
        ("", "value"),
        ("x-tenant", "two\nlines"),
    ] {
        let read = configured_header(name, value);
        assert!(read.is_none(), "{name:?}: {value:?}");
    }
}

// A request that cannot be built never left: it fails the same however
// many times it is tried, so it is final, and no wait is taken for it.
#[tokio::test]
async fn a_request_that_cannot_be_built_is_final() {
    // A URL longer than a request line holds.
    let long = format!("http://127.0.0.1:9/{}{PATH}", "a".repeat(70_000));
    let url = reqwest::Url::parse(&long).unwrap();
    let mut client = client(usual(), 3);
    let waits = Arc::new(Mutex::new(0));
    let kept = Arc::clone(&waits);
    client.set_pace(Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(move |_| {
            *kept.lock().unwrap() += 1;
            Box::pin(std::future::ready(()))
        }),
    });
    let headers = HeaderMap::new();
    let posted = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
    };
    let error = left(&client.send(posted).await.unwrap_err());
    assert_eq!(
        (
            error.message.as_str(),
            error.retryable,
            error.before_request
        ),
        ("the request could not be built", false, true)
    );
    assert_eq!(*waits.lock().unwrap(), 0);
    let asked = Call {
        url: &url,
        headers: &headers,
        body: None,
    };
    let listed = left(&client.send(asked).await.unwrap_err());
    assert_eq!((listed.retryable, listed.before_request), (false, true));
}

mod bounds;
mod tries;
