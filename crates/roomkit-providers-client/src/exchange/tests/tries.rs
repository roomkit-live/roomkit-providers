//! The tries of a request: what is tried again, after what wait, and what
//! a body lost while it is read does to its request.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;

use crate::harness::fake::{Body, Ending, Fake, Reply};
use crate::testing::{client, left, url_on, usual};
use crate::{Call, Client, Pace};

/// The path a test asks.
const PATH: &str = "/v1/chat/completions";

/// A client that tries a failed request `max_retries` times again and
/// takes no wait: each is kept, in what is returned beside it.
fn keeping_its_waits(max_retries: u32) -> (Client, Arc<Mutex<Vec<Duration>>>) {
    let mut client = client(usual(), max_retries);
    let waits = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&waits);
    client.set_pace(Pace {
        now: Arc::new(|| 0.0),
        wait: Arc::new(move |wait| {
            kept.lock().unwrap().push(wait);
            Box::pin(std::future::ready(()))
        }),
    });
    (client, waits)
}

/// An answer of HTTP `status` whose body starts, then is lost with its
/// connection.
fn lost(status: u16) -> Reply {
    Reply::Answer {
        status,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Body::Reads(vec![b"{\"error\": ".to_vec()], Ending::Lost),
    }
}

// A whole answer is read inside its try: a body lost while it is read is
// tried again, that of a refusal as that of an answer.
#[tokio::test]
async fn a_body_lost_while_an_answer_is_read_whole_is_tried_again() {
    for status in [200, 400] {
        let fake = Fake::serving(vec![lost(status), Reply::json(200, "{}")]).await;
        let (client, waits) = keeping_its_waits(1);
        let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
        let call = Call {
            url: &url,
            headers: &headers,
            body: Some(b"{}"),
            token: None,
        };
        let (body, json) = client.send(call).await.unwrap();
        assert_eq!(
            (body.as_slice(), json),
            (b"{}".as_slice(), true),
            "{status}"
        );
        let tried = (fake.seen().len(), waits.lock().unwrap().len());
        assert_eq!(tried, (2, 1), "{status}");
    }
}

// A stream is tried until the head of its answer came, never after: the
// body of a refusal is read outside its tries, as Python's client reads
// it, and one lost while it is read fails the request with no other try.
#[tokio::test]
async fn the_body_of_a_refused_stream_lost_while_it_is_read_is_not_tried_again() {
    let fake = Fake::serving(vec![lost(400), Reply::json(200, "{}")]).await;
    let (client, waits) = keeping_its_waits(1);
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    let error = left(&client.stream(call).await.unwrap_err());
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the connection to the provider failed", true, None)
    );
    let tried = (fake.seen().len(), waits.lock().unwrap().len());
    assert_eq!(tried, (1, 0));
}

// A stream has as many tries as an answer asked whole: its first try is
// tried again after the first backoff, half a second or up to a quarter
// less.
#[tokio::test]
async fn a_stream_is_tried_again_until_the_head_of_its_answer_came() {
    let stream = Reply::stream(vec![b"data: one\n\n".to_vec()], Ending::Ends);
    let fake = Fake::serving(vec![Reply::json(500, "{}"), stream]).await;
    let (client, waits) = keeping_its_waits(1);
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    let mut streamed = client.stream(call).await.unwrap();
    assert_eq!(streamed.next().await.unwrap().as_deref(), Some("one"));
    let waits = waits.lock().unwrap().clone();
    assert_eq!(waits.len(), 1);
    assert!((0.375..=0.5).contains(&waits[0].as_secs_f64()), "{waits:?}");
}

// The wait an answer asks for is the one taken before the next try.
#[tokio::test]
async fn the_wait_an_answer_asks_for_is_the_one_taken() {
    let asking = Reply::Answer {
        status: 429,
        headers: vec![("retry-after".to_owned(), "3".to_owned())],
        body: Body::Whole(Vec::new()),
    };
    let fake = Fake::serving(vec![asking, Reply::json(200, "{}")]).await;
    let (client, waits) = keeping_its_waits(1);
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = Call {
        url: &url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    client.send(call).await.unwrap();
    assert_eq!(*waits.lock().unwrap(), [Duration::from_secs(3)]);
}

// What a failed transport was is kept for the provider: a request whose
// connection could not be made never left; one whose answer was lost had
// left (RFC §25.2).
#[tokio::test]
async fn a_transport_failure_says_whether_its_request_left() {
    let fake = Fake::serving(vec![lost(200)]).await;
    let (client, _) = keeping_its_waits(0);
    let (url, headers) = (url_on(&fake, PATH), HeaderMap::new());
    let call = |url| Call {
        url,
        headers: &headers,
        body: Some(b"{}"),
        token: None,
    };
    let lost = client.send(call(&url)).await.unwrap_err();
    let left = crate::Transport {
        unsent: false,
        timed_out: false,
    };
    assert_eq!(lost.transport_of(), Some(left));
    // A port nothing listens on refuses the connection.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed = listener.local_addr().unwrap();
    drop(listener);
    let refused_url = reqwest::Url::parse(&format!("http://{closed}{PATH}")).unwrap();
    let refused = client.send(call(&refused_url)).await.unwrap_err();
    let unsent = crate::Transport {
        unsent: true,
        timed_out: false,
    };
    assert_eq!(refused.transport_of(), Some(unsent));
}
