//! What a stream does beside reading its events: it starts when it is
//! polled, stops when it is dropped, and ends at its bounds.

use std::time::Duration;

use roomkit_providers_client::stream::STREAM_LIMIT;
use roomkit_providers_llm::{AiProvider, StreamEvent, next};
use serde_json::json;

use crate::OpenAiConfig;
use crate::ported::support::{
    chunk, elsewhere, hello, pointed, sse_of, stream_reply, streamed, text_of,
};
use crate::replay::fake::{Ending, Fake, Reply};
use crate::replay::{drained, let_pass, poll_once};

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

// A stream that stalls is ended by the configuration's read timeout, and
// by no other time: the events that came are handed on, then the failure,
// worth a retry. Nothing here waits on the timeout itself: the first event
// comes in its own time, then the timeout's time is let pass at once.
#[tokio::test]
async fn a_stream_that_stalls_ends_at_the_read_timeout() {
    const TIMEOUT: Duration = Duration::from_secs(40);
    let fake = Fake::serving(vec![stream_reply(vec![said("Hel")], Ending::Silent)]).await;
    let config = OpenAiConfig {
        timeout: TIMEOUT,
        ..elsewhere("local-model")
    };
    let provider = pointed(config, &fake);
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    let first = next(&mut stream).await;
    assert!(
        matches!(&first, Some(Ok(StreamEvent::TextDelta(delta))) if delta.text == "Hel"),
        "{first:?}"
    );
    // The read that waits for more begins here. Half the timeout later it
    // still waits.
    let mut more = std::pin::pin!(next(&mut stream));
    assert!(poll_once(&mut more).await.is_pending());
    let_pass(TIMEOUT / 2).await;
    assert!(poll_once(&mut more).await.is_pending());
    // Once the timeout passed it fails, long before another half of it.
    let_pass(TIMEOUT / 2 + Duration::from_secs(1)).await;
    let failed = tokio::select! {
        biased;
        failed = &mut more => failed,
        () = tokio::time::sleep(TIMEOUT / 2) => panic!("the stream outlives its timeout"),
    };
    let error = failed.unwrap().unwrap_err();
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
    assert_eq!(provider.client.stream_limit(), STREAM_LIMIT);
    provider.client.set_stream_limit(10 * reads[0].len());
    let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
    assert_eq!(text_of(&events), "Hello.".repeat(10));
    let error = failure.unwrap();
    let said = format!(
        "the provider's answer is longer than {} bytes",
        provider.client.stream_limit()
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

// What the reader read before it fails is handed on, however the answer is
// cut: an event and a line that is no UTF-8 in one read, or in two.
#[tokio::test]
async fn the_events_read_before_a_line_that_does_not_read_are_handed_on() {
    let first = said("Hel");
    let bad = b"data: \xff\xfe\n\ndata: late\n\n".to_vec();
    for reads in [
        vec![[first.clone(), bad.clone()].concat()],
        vec![first, bad],
    ] {
        let (events, failure) = streamed(stream_reply(reads, Ending::Ends), &hello()).await;
        assert_eq!(text_of(&events), "Hel");
        let error = failure.unwrap();
        assert_eq!(
            (error.message.as_str(), error.retryable, error.status_code),
            (
                "the provider's answer is malformed: an event's data is not UTF-8",
                false,
                None
            )
        );
    }
}

/// One read of a stream that holds `events`, each an event's data, and
/// nothing after them.
fn read_of(events: &[String]) -> Vec<u8> {
    let events = events.iter().map(|data| format!("data: {data}\n\n"));
    events.collect::<String>().into_bytes()
}

/// A chunk that ends an answer for `finish_reason`.
fn finish(finish_reason: &str) -> String {
    json!({"choices": [{"index": 0, "delta": {}, "finish_reason": finish_reason}]}).to_string()
}

/// A chunk that writes a whole call to `lookup`.
fn call() -> String {
    let function = json!({"name": "lookup", "arguments": "{\"number\": \"A-1\"}"});
    chunk(&json!({"tool_calls": [{"index": 0, "id": "call_1", "function": function}]})).to_string()
}

// Not in Python, which reads such a stream as complete: a stream that ends
// with neither a finish reason nor its end mark was cut. What came is
// handed on, then it fails as a stream whose connection is lost does; a
// call it brought is never handed whole, to run.
#[tokio::test]
async fn a_stream_that_ends_with_no_finish_reason_and_no_end_mark_was_cut() {
    let said = chunk(&json!({"content": "Hello."})).to_string();
    for (reads, text, deltas) in [
        (vec![read_of(std::slice::from_ref(&said))], "Hello.", 0),
        (vec![read_of(&[call()])], "", 1),
        (vec![read_of(&[said.clone(), call()])], "Hello.", 1),
        // A partial tag the answer ends on is text that came.
        (
            vec![read_of(&[
                chunk(&json!({"content": "Hello.<thi"})).to_string()
            ])],
            "Hello.<thi",
            0,
        ),
    ] {
        let (events, failure) = streamed(stream_reply(reads, Ending::Ends), &hello()).await;
        assert_eq!(text_of(&events), text);
        let composed = events
            .iter()
            .filter(|event| matches!(event, StreamEvent::ToolCallDelta(_)));
        assert_eq!(composed.count(), deltas);
        let ended =
            |event: &StreamEvent| matches!(event, StreamEvent::ToolCall(_) | StreamEvent::Done(_));
        assert!(!events.iter().any(ended), "{events:?}");
        let error = failure.unwrap();
        assert_eq!(
            error.message,
            "the provider's stream ended with neither a finish reason nor its end mark"
        );
        let attributes = (error.retryable, error.status_code, error.before_request);
        assert_eq!(attributes, (true, None, false));
    }
    // The text of such a stream fails the same way, after what came.
    let fake = Fake::serving(vec![stream_reply(vec![read_of(&[said])], Ending::Ends)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let (pieces, failure) = drained(provider.generate_stream(&hello())).await;
    assert_eq!(pieces, ["Hello."]);
    assert!(failure.unwrap().retryable);
}

// A stream that ends with a finish reason and no end mark, or with its end
// mark and no finish reason, is complete: its calls and its end are handed
// on, as Python hands them.
#[tokio::test]
async fn a_stream_that_ends_with_a_finish_reason_or_with_its_end_mark_is_complete() {
    let said = chunk(&json!({"content": "Hello."})).to_string();
    let mark = "[DONE]".to_owned();
    for (events, finish_reason, calls) in [
        (vec![said.clone(), finish("stop")], Some("stop"), 0),
        (vec![said.clone(), mark.clone()], None, 0),
        (vec![call(), finish("tool_calls")], Some("tool_calls"), 1),
        (vec![call(), mark], None, 1),
        // A finish reason that ends the answer in an event its end cuts.
        (vec![said, finish("length")], Some("length"), 0),
    ] {
        let reply = stream_reply(vec![read_of(&events)], Ending::Ends);
        let (given, failure) = streamed(reply, &hello()).await;
        assert!(failure.is_none(), "{events:?}: {failure:?}");
        let Some(StreamEvent::Done(done)) = given.last() else {
            panic!("{events:?}: no end in {given:?}");
        };
        assert_eq!(done.finish_reason.as_deref(), finish_reason, "{events:?}");
        let whole = given.iter().filter_map(|event| match event {
            StreamEvent::ToolCall(call) => Some(call.partial),
            StreamEvent::TextDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::Usage(_) => None,
        });
        assert_eq!(whole.collect::<Vec<_>>(), vec![false; calls], "{events:?}");
    }
}

// Python keeps a finish reason only when it is not empty: an empty one says
// nothing of how the answer ended. With its end mark such a stream is
// complete and has no reason; without it, it was cut.
#[tokio::test]
async fn an_empty_finish_reason_is_none() {
    let said = chunk(&json!({"content": "Hello."})).to_string();
    let marked = vec![said.clone(), finish(""), "[DONE]".to_owned()];
    let reply = stream_reply(vec![read_of(&marked)], Ending::Ends);
    let (given, failure) = streamed(reply, &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    let Some(StreamEvent::Done(done)) = given.last() else {
        panic!("no end in {given:?}");
    };
    assert_eq!(done.finish_reason, None);

    let reply = stream_reply(vec![read_of(&[said, finish("")])], Ending::Ends);
    let (given, failure) = streamed(reply, &hello()).await;
    assert_eq!(
        failure.unwrap().message,
        "the provider's stream ended with neither a finish reason nor its end mark"
    );
    assert!(
        matches!(given.last(), Some(StreamEvent::TextDelta(_))),
        "{given:?}"
    );
}

// Not in Python, which says such an answer carried no event: a JSON error
// object that answers a stream request with a 200 reads as the status it
// names, as `generate` reads the same body. No head of a stream came: the
// failure says whether a new request may succeed.
#[tokio::test]
async fn a_json_error_object_answering_a_stream_reads_as_the_status_it_names() {
    let overloaded =
        json!({"error": {"message": "The engine is overloaded.", "type": "overloaded_error"}});
    let refused = json!({"error": {"message": "No such model.", "code": 404}});
    for (body, status, retryable) in [(&overloaded, 529, true), (&refused, 404, false)] {
        let reply = || crate::ported::support::json_reply(200, body.to_string());
        let (events, failure) = streamed(reply(), &hello()).await;
        assert!(events.is_empty(), "{events:?}");
        let streamed = failure.unwrap();
        let whole = crate::ported::support::asked(reply(), &hello())
            .await
            .unwrap_err();
        let attributes = |error: &roomkit_providers_llm::ProviderError| {
            (
                error.message.clone(),
                error.status_code,
                error.retryable,
                error.before_request,
            )
        };
        assert_eq!(attributes(&streamed), attributes(&whole), "{body}");
        assert_eq!(
            (streamed.status_code, streamed.retryable),
            (Some(status), retryable)
        );
    }
    // A JSON body that holds no error, and an error object the server does
    // not say is JSON, are an answer that carried no event, as before.
    let no_event = "The provider's stream carried no event: its body could not be read";
    let json_reply = crate::ported::support::json_reply;
    let plain = Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/plain".to_owned())],
        body: crate::replay::fake::Body::Whole(overloaded.to_string().into_bytes()),
    };
    let empty = json!({"error": null, "status": "ok"}).to_string();
    for reply in [
        json_reply(200, empty),
        json_reply(200, "[]".to_owned()),
        json_reply(200, "{".to_owned()),
        plain,
    ] {
        let (events, failure) = streamed(reply, &hello()).await;
        assert!(events.is_empty(), "{events:?}");
        let error = failure.unwrap();
        assert_eq!((error.message.as_str(), error.retryable), (no_event, false));
    }
}

// Not in Python, whose client reads the line a byte order mark starts as no
// data: the mark that starts a stream is ignored, as the format of a stream
// of events says, and its first event is read.
#[tokio::test]
async fn a_byte_order_mark_that_starts_a_stream_is_ignored() {
    let said = chunk(&json!({"content": "Hello."})).to_string();
    let body = [&b"\xEF\xBB\xBF"[..], &read_of(&[said, finish("stop")])].concat();
    // In one read, and with the mark cut from what follows it.
    for reads in [
        vec![body.clone()],
        vec![body[..2].to_vec(), body[2..].to_vec()],
    ] {
        let (events, failure) = streamed(stream_reply(reads, Ending::Ends), &hello()).await;
        assert!(failure.is_none(), "{failure:?}");
        assert_eq!(text_of(&events), "Hello.");
        assert!(matches!(events.last(), Some(StreamEvent::Done(_))));
    }
}
