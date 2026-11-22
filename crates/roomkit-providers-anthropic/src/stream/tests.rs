//! What a stream does beside reading its events: it starts when it is
//! polled, stops when it is dropped, and ends at its bounds.

use std::time::Duration;

use roomkit_ai::{AiProvider, ProviderError, StreamEvent};
use roomkit_providers_client::ANSWER_LIMIT;
use roomkit_providers_client::items::next;
use roomkit_providers_client::stream::STREAM_LIMIT;
use serde_json::{Value, json};

use crate::AnthropicConfig;
use crate::replay::fake::{Body, Ending, Fake, Reply};
use crate::replay::{drained, let_pass, poll_once};
use crate::tests::support::{
    elsewhere, end, event, hello, message_saying, pointed, sse_of, start, stream_of, streamed,
    text_block, text_of,
};

/// One read of a stream: a message that starts and says `text`, with no
/// end after it.
fn said(text: &str) -> Vec<u8> {
    let [opened, delta, _] = text_block(0, text);
    sse_of(&[start(), opened, delta])
}

/// One read that goes on with the text of the block [`said`] opened.
fn more(text: &str) -> Vec<u8> {
    let [_, delta, _] = text_block(0, text);
    sse_of(&[delta])
}

/// A stream that says `Hel`, then keeps its connection open and silent.
fn stalling() -> Reply {
    Reply::stream(vec![said("Hel")], Ending::Silent)
}

/// Waits until `fake` saw its client close the connection it kept open.
/// The close is awaited, not polled for; the deadline only turns a
/// connection that is never closed into a failure.
async fn closed(fake: &Fake) {
    let seen = tokio::time::timeout(Duration::from_secs(30), fake.closed()).await;
    assert!(seen.is_ok(), "the connection was never closed");
}

/// The next piece of text of `stream`, past the events that are none.
async fn next_text(stream: &mut roomkit_ai::ProviderStream<'_>) -> Option<String> {
    while let Some(item) = next(stream).await {
        if let Ok(StreamEvent::TextDelta(delta)) = item {
            return Some(delta.text);
        }
    }
    None
}

// Cancellation: a stream that is dropped stops its request. The fake keeps
// the connection open after the first text; it sees its client close it.
#[tokio::test]
async fn dropping_a_stream_closes_its_connection() {
    let fake = Fake::serving(vec![stalling()]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    assert_eq!(next_text(&mut stream).await.as_deref(), Some("Hel"));
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
    let fake = Fake::serving(vec![stalling()]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let mut stream = provider.generate_stream(&request);
    assert_eq!(next(&mut stream).await.unwrap().unwrap(), "Hel");
    drop(stream);
    closed(&fake).await;
}

// Source: tests/transport.rs::cancelling_an_active_stream_closes_its_connection
// (there the server waits three seconds for the close; here the close is
// awaited once the fake said its answer went silent)
// Python: src/roomkit/providers/anthropic/ai.py::AnthropicAIProvider._events, which
// leaves `messages.stream()` when its reader stops.
// The same of a whole answer, which is folded from the stream: a
// generation that is cancelled stops its request.
#[tokio::test]
async fn cancelling_a_generation_closes_its_connection() {
    let fake = Fake::serving(vec![stalling()]).await;
    let config = AnthropicConfig {
        max_retries: 3,
        ..elsewhere("local-model")
    };
    let provider = pointed(config, &fake);
    let generation = tokio::spawn(async move { provider.generate(&hello()).await });
    let silent = tokio::time::timeout(Duration::from_secs(30), fake.silent()).await;
    assert!(silent.is_ok(), "the answer never went silent");
    generation.abort();
    assert!(generation.await.unwrap_err().is_cancelled());
    closed(&fake).await;
    // Nothing was tried again.
    assert_eq!(fake.seen().len(), 1);
}

// A stream is pulled: making it sends nothing, and one that is never polled
// never asks.
#[tokio::test]
async fn nothing_is_sent_before_a_stream_is_polled() {
    let fake = Fake::serving(vec![Reply::json(200, r#"{"data": []}"#)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let request = hello();
    let stream = provider.generate_structured_stream(&request);
    let text = provider.generate_stream(&request);
    // A whole exchange with the same fake comes and goes meanwhile.
    assert_eq!(provider.list_models().await.unwrap().len(), 0);
    assert_eq!(fake.seen().len(), 1);
    assert_eq!(fake.seen()[0].method, "GET");
    drop((stream, text));
    assert_eq!(fake.seen().len(), 1);
}

// A stream that stalls is ended by the configuration's read timeout, and
// by no other time: the events that came are handed on, then the failure,
// worth a retry. Nothing here waits on the timeout itself: the first text
// comes in its own time, then the timeout's time is let pass at once.
#[tokio::test]
async fn a_stream_that_stalls_ends_at_the_read_timeout() {
    const TIMEOUT: Duration = Duration::from_secs(40);
    let fake = Fake::serving(vec![stalling()]).await;
    let config = AnthropicConfig {
        timeout: TIMEOUT,
        ..elsewhere("local-model")
    };
    let provider = pointed(config, &fake);
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    assert_eq!(next_text(&mut stream).await.as_deref(), Some("Hel"));
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
    let first = said("Hello.");
    let reads: Vec<Vec<u8>> = std::iter::once(first.clone())
        .chain((0..64).map(|_| more("Hello.")))
        .collect();
    let fake = Fake::serving(vec![Reply::stream(reads.clone(), Ending::Ends)]).await;
    let mut provider = pointed(elsewhere("local-model"), &fake);
    assert_eq!(provider.client.stream_limit(), STREAM_LIMIT);
    provider
        .client
        .set_stream_limit(first.len() + 9 * reads[1].len());
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

// An answer read whole, the model listing, past the most bytes read of one
// fails the request, final: 4 MiB.
#[tokio::test]
async fn a_listing_past_the_bound_of_an_answer_fails() {
    assert_eq!(ANSWER_LIMIT, 4 * 1024 * 1024);
    let page = format!(r#"{{"data": []{}}}"#, " ".repeat(ANSWER_LIMIT));
    let within = format!(r#"{{"data": []{}}}"#, " ".repeat(ANSWER_LIMIT - 12));
    assert_eq!(within.len(), ANSWER_LIMIT);
    let fake = Fake::serving(vec![Reply::json(200, page), Reply::json(200, within)]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let error = provider.list_models().await.unwrap_err();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        (
            "The provider answered with a body it could not read: the provider's answer is \
             longer than 4194304 bytes",
            false,
            None
        )
    );
    assert_eq!(provider.list_models().await.unwrap().len(), 0);
}

// A ping, a comment and an event with no data say nothing, and what
// follows the event that ends the message is not read: neither an event
// that would fail the stream, nor the rest of the connection.
#[tokio::test]
async fn the_end_of_the_message_ends_the_reading() {
    let mut body =
        b": connected\n\nevent: ping\n\nevent: ping\ndata: {\"type\": \"ping\"}\n\n".to_vec();
    body.extend(sse_of(&message_saying("Hello.")));
    body.extend(b"event: content_block_delta\ndata: {not json\n\n");
    let late = json!({"type": "error", "error": {"type": "overloaded_error"}});
    let reply = Reply::stream(vec![body, sse_of(&[late])], Ending::Silent);
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
        let (events, failure) = streamed(Reply::stream(reads, Ending::Ends), &hello()).await;
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

/// A block that calls `lookup` at `index`, its arguments whole, and never
/// closes.
fn open_call(index: u32) -> [Value; 2] {
    let block = json!({"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}});
    let fragment = json!({"type": "input_json_delta", "partial_json": "{\"number\": \"A-1\"}"});
    [
        json!({"type": "content_block_start", "index": index, "content_block": block}),
        json!({"type": "content_block_delta", "index": index, "delta": fragment}),
    ]
}

// Not in Python, which reads such a stream as complete: a stream that ends
// with neither a stop reason nor the event that ends its message was cut.
// What came is handed on, then it fails as a stream whose connection is
// lost does. A call whose block closed is part of what came, handed as it
// came; a call still open is never handed. No done event follows: the
// failure is the stream's last item.
#[tokio::test]
async fn a_stream_that_ends_with_no_stop_reason_and_no_end_was_cut() {
    let [opened, delta, stopped] = text_block(0, "Hello.");
    let closed = json!({"type": "content_block_stop", "index": 0});
    for (events, text, composed, handed) in [
        (vec![start()], "", 0, 0),
        (vec![start(), opened.clone(), delta.clone()], "Hello.", 0, 0),
        (vec![start(), opened, delta, stopped], "Hello.", 0, 0),
        ([&[start()][..], &open_call(0)].concat(), "", 2, 0),
        (
            [&[start()][..], &open_call(0), &[closed]].concat(),
            "",
            2,
            1,
        ),
    ] {
        let (given, failure) = streamed(stream_of(&events), &hello()).await;
        assert_eq!(text_of(&given), text);
        let deltas = given
            .iter()
            .filter(|event| matches!(event, StreamEvent::ToolCallDelta(_)));
        assert_eq!(deltas.count(), composed);
        let calls = given.iter().filter_map(|event| match event {
            StreamEvent::ToolCall(call) => Some((call.id.as_str(), call.partial, call.garbled)),
            StreamEvent::TextDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::Usage(_) => None,
        });
        let whole = [("toolu_1", false, false)];
        assert_eq!(calls.collect::<Vec<_>>(), whole[..handed], "{given:?}");
        let ended = |event: &StreamEvent| matches!(event, StreamEvent::Done(_));
        assert!(!given.iter().any(ended), "{given:?}");
        let error = failure.unwrap();
        assert_eq!(
            error.message,
            "the provider's stream ended with neither a stop reason nor the end of its message"
        );
        let attributes = (error.retryable, error.status_code, error.before_request);
        assert_eq!(attributes, (true, None, false));
    }
    // The text of such a stream fails the same way, after what came, and a
    // whole answer gives nothing of it.
    let cut = || Reply::stream(vec![said("Hello.")], Ending::Ends);
    let fake = Fake::serving(vec![cut(), cut()]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let (pieces, failure) = drained(provider.generate_stream(&hello())).await;
    assert_eq!(pieces, ["Hello."]);
    assert!(failure.unwrap().retryable);
    assert!(provider.generate(&hello()).await.unwrap_err().retryable);
}

/// What a stream of events gives of each of `bodies`, each the one read of
/// an answer that then ends: its events, then the failure that ended them.
async fn streamed_each(bodies: Vec<Vec<u8>>) -> Vec<(Vec<StreamEvent>, Option<ProviderError>)> {
    let asked = bodies.len();
    let replies = bodies.into_iter().map(|body| stream_of_bytes(vec![body]));
    let fake = Fake::serving(replies.collect()).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let mut given = Vec::new();
    for _ in 0..asked {
        given.push(drained(provider.generate_structured_stream(&hello())).await);
    }
    given
}

/// An answer sent in `reads`, after which the stream ends.
fn stream_of_bytes(reads: Vec<Vec<u8>>) -> Reply {
    Reply::stream(reads, Ending::Ends)
}

/// `before`, then each start of `event` that stops short of its blank
/// line: the bodies of answers that end inside `event`, and one that ends
/// before it.
fn ending_inside(before: &[u8], event: &str) -> Vec<Vec<u8>> {
    let cuts = (0..event.len()).map(|at| &event.as_bytes()[..at]);
    cuts.map(|start| [before, start].concat()).collect()
}

/// What the failure of a stream that was cut says.
const CUT: &str =
    "the provider's stream ended with neither a stop reason nor the end of its message";

// A body that ends inside an event was cut there, whatever of the event
// came: the event is not read, and the stream is what it was before it.
// With no stop reason before it, what came is handed on, then the stream
// fails as one that ended with neither a stop reason nor its end, worth a
// retry, never in the words of a reader of the event. An event whose data
// came whole is read, with no blank line and no line end after it.
#[tokio::test]
async fn a_stream_that_ends_inside_an_event_was_cut_before_it() {
    let [_, more, _] = text_block(0, "lo.");
    let (before, next) = (said("Hel"), event(&more));
    // The data of the event is whole two bytes before its end: its line
    // end, then its blank line.
    let whole = next.len() - 2;
    let given = streamed_each(ending_inside(&before, &next)).await;
    for (at, (events, failure)) in given.iter().enumerate() {
        let text = if at < whole { "Hel" } else { "Hello." };
        assert_eq!(text_of(events), text, "cut at {at}");
        let done = |event: &StreamEvent| matches!(event, StreamEvent::Done(_));
        assert!(!events.iter().any(done), "cut at {at}: {events:?}");
        let error = failure.as_ref().unwrap();
        assert_eq!(error.message, CUT, "cut at {at}");
        let attributes = (error.retryable, error.status_code, error.before_request);
        assert_eq!(attributes, (true, None, false), "cut at {at}");
    }
    // A body that ends inside a character ends inside an event.
    let [_, wide, _] = text_block(0, "caf\u{e9}");
    let wide = event(&wide);
    let inside = wide.find('\u{e9}').unwrap() + 1;
    let cut = [&before[..], &wide.as_bytes()[..inside]].concat();
    // A whole answer and the text of such a stream fail the same way.
    let replies = (0..3).map(|_| stream_of_bytes(vec![cut.clone()]));
    let fake = Fake::serving(replies.collect()).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
    assert_eq!(text_of(&events), "Hel");
    let (pieces, text_failure) = drained(provider.generate_stream(&hello())).await;
    assert_eq!(pieces, ["Hel"]);
    let whole_answer = provider.generate(&hello()).await.unwrap_err();
    for error in [failure.unwrap(), text_failure.unwrap(), whole_answer] {
        assert_eq!((error.message.as_str(), error.retryable), (CUT, true));
    }
}

// Once a stream said why it stopped it is complete, as Python reads it,
// wherever the event that ends its message is cut.
#[tokio::test]
async fn a_stream_that_ends_inside_the_event_after_its_stop_reason_is_complete() {
    let events = message_saying("Hello.");
    let (said, stop) = events.split_at(events.len() - 1);
    let given = streamed_each(ending_inside(&sse_of(said), &event(&stop[0]))).await;
    for (at, (events, failure)) in given.iter().enumerate() {
        assert!(failure.is_none(), "cut at {at}: {failure:?}");
        assert_eq!(text_of(events), "Hello.", "cut at {at}");
        let Some(StreamEvent::Done(done)) = events.last() else {
            panic!("cut at {at}: no end in {events:?}");
        };
        assert_eq!(done.finish_reason.as_deref(), Some("end_turn"));
        assert_eq!(done.usage["output_tokens"], 5, "cut at {at}");
    }
}

// A body that ends inside its first event holds no event: the failure of an
// answer that carried none, final, as Python reads it. Once the data of the
// message's start came whole, the message started, and was cut.
#[tokio::test]
async fn a_stream_that_ends_inside_its_first_event_carried_no_event() {
    let first = event(&start());
    let whole = first.len() - 2;
    let given = streamed_each(ending_inside(b"", &first)).await;
    for (at, (events, failure)) in given.iter().enumerate() {
        let error = failure.as_ref().unwrap();
        let said = (error.message.as_str(), error.retryable, events.len());
        if at < whole {
            let no_event = "The provider's stream carried no event: its body could not be read";
            assert_eq!(said, (no_event, false, 0), "cut at {at}");
        } else {
            assert_eq!(said, (CUT, true, 1), "cut at {at}");
        }
    }
}

// A stream that ends with a stop reason and no end event, or with its end
// event and no stop reason, is complete: its calls and its end are handed
// on, as Python hands them. A call left open is read for how the message
// stopped.
#[tokio::test]
async fn a_stream_that_ends_with_a_stop_reason_or_with_its_end_is_complete() {
    let [reason, stop] = end("tool_use");
    let cut = json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}});
    let unsaid = json!({"type": "message_delta", "delta": {"stop_reason": null}});
    for (last, finish_reason, partial) in [
        (vec![reason.clone()], Some("tool_use"), false),
        (vec![reason, stop.clone()], Some("tool_use"), false),
        (vec![stop.clone()], None, false),
        (vec![unsaid, stop], None, false),
        (vec![cut], Some("max_tokens"), false),
    ] {
        let events = [&[start()][..], &open_call(0), &last].concat();
        let (given, failure) = streamed(stream_of(&events), &hello()).await;
        assert!(failure.is_none(), "{last:?}: {failure:?}");
        let Some(StreamEvent::Done(done)) = given.last() else {
            panic!("{last:?}: no end in {given:?}");
        };
        assert_eq!(done.finish_reason.as_deref(), finish_reason, "{last:?}");
        let calls = given.iter().filter_map(|event| match event {
            StreamEvent::ToolCall(call) => Some((call.id.as_str(), call.partial)),
            StreamEvent::TextDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::Usage(_) => None,
        });
        assert_eq!(
            calls.collect::<Vec<_>>(),
            [("toolu_1", partial)],
            "{last:?}"
        );
    }
}

/// What `generate` and its stream give of `reply`: each one's failure.
async fn failures_of(reply: impl Fn() -> Reply) -> [ProviderError; 2] {
    let fake = Fake::serving(vec![reply(), reply()]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let (events, failure) = drained(provider.generate_structured_stream(&hello())).await;
    assert!(events.is_empty(), "{events:?}");
    let whole = provider.generate(&hello()).await.unwrap_err();
    [failure.unwrap(), whole]
}

// Not in Python, which says such an answer carried no event: a JSON error
// object that answers a request with a 200 reads as the status its type
// names, as the same error told as an event of a stream does. No head of
// a stream came: the failure says whether a new request may succeed.
#[tokio::test]
async fn a_json_error_object_answering_a_stream_reads_as_the_status_it_names() {
    let error = |kind: &str, message: &str| json!({"type": "error", "error": {"type": kind, "message": message}, "request_id": "req_1"});
    let overloaded = error("overloaded_error", "Overloaded");
    let refused = error("not_found_error", "No such model.");
    for (body, status, retryable) in [(&overloaded, 529, true), (&refused, 404, false)] {
        for failure in failures_of(|| Reply::json(200, body.to_string())).await {
            let said = body["error"]["message"].as_str().unwrap();
            assert_eq!(failure.message, said);
            assert_eq!(
                (
                    failure.status_code,
                    failure.retryable,
                    failure.before_request
                ),
                (Some(status), retryable, false)
            );
        }
    }
    // A JSON body that holds no error, a whole message among them, and an
    // error object the server does not say is JSON, are an answer that
    // carried no event.
    let no_event = "The provider's stream carried no event: its body could not be read";
    let plain = || Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/plain".to_owned())],
        body: Body::Whole(overloaded.to_string().into_bytes()),
    };
    let whole = json!({"type": "message", "content": [{"type": "text", "text": "Hello."}]});
    let replies: [Box<dyn Fn() -> Reply + '_>; 5] = [
        Box::new(|| Reply::json(200, json!({"error": null, "status": "ok"}).to_string())),
        Box::new(|| Reply::json(200, whole.to_string())),
        Box::new(|| Reply::json(200, "[]")),
        Box::new(|| Reply::json(200, "{")),
        Box::new(plain),
    ];
    for reply in replies {
        for failure in failures_of(&reply).await {
            assert_eq!(
                (
                    failure.message.as_str(),
                    failure.retryable,
                    failure.status_code
                ),
                (no_event, false, None)
            );
        }
    }
}

// Not in Python, whose client reads the line a byte order mark starts as no
// event: the mark that starts a stream is ignored, as the format of a
// stream of events says, and its first event is read.
#[tokio::test]
async fn a_byte_order_mark_that_starts_a_stream_is_ignored() {
    let body = [&b"\xEF\xBB\xBF"[..], &sse_of(&message_saying("Hello."))].concat();
    // In one read, and with the mark cut from what follows it.
    for reads in [
        vec![body.clone()],
        vec![body[..2].to_vec(), body[2..].to_vec()],
    ] {
        let (events, failure) = streamed(Reply::stream(reads, Ending::Ends), &hello()).await;
        assert!(failure.is_none(), "{failure:?}");
        assert_eq!(text_of(&events), "Hello.");
        assert!(matches!(events.first(), Some(StreamEvent::Usage(_))));
        assert!(matches!(events.last(), Some(StreamEvent::Done(_))));
    }
}

// An event is read by the type its data names, whatever line names it, and
// one whose data names none cannot be read.
#[tokio::test]
async fn an_event_is_read_by_the_type_its_data_names() {
    let unnamed = message_saying("Hello.");
    let unnamed: Vec<String> = unnamed
        .iter()
        .map(|data| format!("data: {data}\n\n"))
        .collect();
    let reply = Reply::stream(vec![unnamed.concat().into_bytes()], Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(text_of(&events), "Hello.");

    let typeless = format!("{}event: message_stop\ndata: {{}}\n\n", event(&start()));
    let reply = Reply::stream(vec![typeless.into_bytes()], Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert_eq!(events.len(), 1, "what the start told: {events:?}");
    let error = failure.unwrap();
    assert_eq!(
        (error.message.as_str(), error.retryable),
        (
            "The provider answered with a body it could not read: an event of the stream names \
             no type",
            false
        )
    );
}
