//! What a stream does beside reading its events: it starts when it is
//! polled, stops when it is dropped, and ends at its bounds.

use std::time::Duration;

use roomkit_providers_core::{AiProvider, StreamEvent, next};
use roomkit_providers_http::sse::STREAM_LIMIT;
use serde_json::json;

use crate::OpenAiConfig;
use crate::ported::support::{
    chunk, elsewhere, hello, pointed, sse_of, stream_reply, streamed, text_of,
};
use crate::replay::drained;
use crate::replay::fake::{Ending, Fake, Reply};

/// The events of a chunk that says `text`, with no end after it.
fn said(text: &str) -> Vec<u8> {
    format!("data: {}\n\n", chunk(&json!({"content": text}))).into_bytes()
}

/// Waits until `fake` saw its client close the connection it kept open.
/// The close is awaited, not polled for; the deadline only turns a
/// connection that is never closed into a failure.
async fn closed(fake: &Fake) {
    let seen = tokio::time::timeout(Duration::from_secs(30), fake.closed()).await;
    assert!(seen.is_ok(), "the connection was never closed");
}

// Cancellation: a stream that is dropped stops its request. The fake keeps
// the connection open after the first event; it sees its client close it.
#[tokio::test]
async fn dropping_a_stream_closes_its_connection() {
    let fake = Fake::serving(vec![stream_reply(vec![said("Hel")], Ending::Silent)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    let first = next(&mut stream).await;
    assert!(
        matches!(&first, Some(Ok(StreamEvent::TextDelta(delta))) if delta.text == "Hel"),
        "{first:?}"
    );
    drop(stream);
    closed(&fake).await;
}

// The same while the answer has not started: the request was sent, its
// answer does not come, and the stream is dropped.
#[tokio::test]
async fn dropping_a_stream_that_waits_for_its_answer_closes_its_connection() {
    let fake = Fake::serving(vec![Reply::Silent]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    tokio::select! {
        item = next(&mut stream) => panic!("no answer comes: {item:?}"),
        () = fake.received() => {}
    }
    drop(stream);
    closed(&fake).await;
}

// The same of the text stream, which reads the stream of events.
#[tokio::test]
async fn dropping_a_text_stream_closes_its_connection() {
    let fake = Fake::serving(vec![stream_reply(vec![said("Hel")], Ending::Silent)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let mut stream = provider.generate_stream(&request);
    assert_eq!(next(&mut stream).await.unwrap().unwrap(), "Hel");
    drop(stream);
    closed(&fake).await;
}

// A stream is pulled: making it sends nothing, and one that is never polled
// never asks.
#[tokio::test]
async fn nothing_is_sent_before_a_stream_is_polled() {
    let fake = Fake::serving(vec![stream_reply(vec![said("Hel")], Ending::Ends)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let stream = provider.generate_structured_stream(&request);
    // A whole exchange with the same fake comes and goes meanwhile.
    let listing = provider.list_models().await;
    assert!(
        listing.is_err(),
        "the fake answers the listing with the stream"
    );
    assert_eq!(fake.seen().len(), 1);
    assert_eq!(fake.seen()[0].method, "GET");
    drop(stream);
    assert_eq!(fake.seen().len(), 1);
}

// A stream that stalls is ended by the configuration's read timeout: the
// events that came are handed on, then the failure, worth a retry.
#[tokio::test]
async fn a_stream_that_stalls_ends_at_the_read_timeout() {
    let fake = Fake::serving(vec![stream_reply(vec![said("Hel")], Ending::Silent)]).await;
    let config = OpenAiConfig {
        timeout: Duration::from_millis(40),
        ..elsewhere("local-model")
    };
    let provider = pointed(config, &fake);
    let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
    assert_eq!(text_of(&events), "Hel");
    let error = failure.unwrap();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        ("the provider did not answer in time", true, None)
    );
    // The provider let go of the connection.
    closed(&fake).await;
}

// A stream past the most bytes read of one fails, final: what was read
// before the bound is handed on first. The bound is the reader's, 256 MiB,
// lowered here.
#[tokio::test]
async fn a_stream_past_its_whole_bound_fails() {
    assert_eq!(STREAM_LIMIT, 256 * 1024 * 1024);
    let reads: Vec<Vec<u8>> = (0..64).map(|_| said("Hello.")).collect();
    let fake = Fake::serving(vec![stream_reply(reads.clone(), Ending::Ends)]).await;
    let mut provider = pointed(elsewhere("local-model"), &fake);
    assert_eq!(provider.stream_limit, STREAM_LIMIT);
    provider.stream_limit = 10 * reads[0].len();
    let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
    assert_eq!(text_of(&events), "Hello.".repeat(10));
    let error = failure.unwrap();
    let said = format!(
        "the provider's answer is longer than {} bytes",
        provider.stream_limit
    );
    assert_eq!(
        (error.message, error.retryable, error.status_code),
        (said, false, None)
    );
}

// An event with no data line, as a server keeps a connection alive with, is
// no event: the format's rule. A comment line is none either.
#[tokio::test]
async fn an_event_with_no_data_is_skipped() {
    let chunks = [chunk(&json!({"content": "Hello."}))];
    let body = format!(": keep-alive\n\nevent: ping\n\n{}", sse_of(&chunks));
    let reply = stream_reply(vec![body.into_bytes()], Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(text_of(&events), "Hello.");
}

// What follows the end mark on its line is not read, as Python's client
// reads a line that starts with the mark; nor is what follows the line.
#[tokio::test]
async fn the_end_mark_ends_the_reading() {
    let first = chunk(&json!({"content": "Hello."}));
    let late = chunk(&json!({"content": " More."}));
    let body = format!("data: {first}\n\ndata: [DONE] not json\n\ndata: {late}\n\n");
    let reply = stream_reply(vec![body.into_bytes()], Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(text_of(&events), "Hello.");
    assert!(matches!(events.last(), Some(StreamEvent::Done(_))));
}
