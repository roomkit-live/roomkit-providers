//! The times a request is given, and what a request that never left reads
//! as. No test here waits on a timeout: the time is let pass at once, after
//! what must have happened first is seen to have happened.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;

use super::{Call, Pace};
use crate::Timeouts;
use crate::harness::fake::{Fake, Reply};
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
    let timeouts = Timeouts::new(TIMEOUT, usual().connect).unwrap();
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
