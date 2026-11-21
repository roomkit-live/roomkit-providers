//! A streamed answer: its reasoning, its check against a response schema,
//! its timeout and its bounds.

use std::sync::Arc;

use roomkit_providers_llm::{AiProvider, AiRequest, ResponseSchemaFailure, StreamEvent, next};
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::support::{
    chunk, elsewhere, ending, events, hello, pointed, schema_failure, sse_of, stream_reply,
    streamed, text_of,
};
use crate::replay::fake::{Body, Ending, Fake, Reply};
use crate::replay::{drained, let_pass, poll_once};

/// More bytes than a line or an event of a stream may hold, and than an
/// answer read whole may weigh.
const TOO_LONG: usize = 5 * 1024 * 1024;

/// A piece of a stream, as the provider hands it.
#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Thinking(String),
    Text(String),
}

fn thinking(text: &str) -> Piece {
    Piece::Thinking(text.to_owned())
}

fn text(text: &str) -> Piece {
    Piece::Text(text.to_owned())
}

/// The reasoning and the text the deltas of a stream give, in order.
async fn pieces(deltas: &[Value]) -> Vec<Piece> {
    let chunks: Vec<Value> = deltas.iter().map(chunk).collect();
    let (events, failure) = streamed(events(&chunks), &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    let pieces = events.into_iter().filter_map(|event| match event {
        StreamEvent::ThinkingDelta(delta) => Some(Piece::Thinking(delta.thinking)),
        StreamEvent::TextDelta(delta) => Some(Piece::Text(delta.text)),
        StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::Done(_)
        | StreamEvent::Usage(_) => None,
    });
    pieces.collect()
}

// Source: tests/agent/thinking.rs::a_think_block_reads_the_same_streamed_or_not
// (the answer streamed; asked whole, it is in `answers`)
#[tokio::test]
async fn a_think_block_and_its_answer_keep_their_spaces_streamed() {
    for (content, reasoning, answer) in [
        (
            "<think> weighing it \n</think>\n\nThe answer.  ",
            " weighing it \n",
            "\n\nThe answer.  ",
        ),
        ("<think>x</think>\n\n", "x", "\n\n"),
    ] {
        let read = pieces(&[json!({"content": content})]).await;
        assert_eq!(read, [thinking(reasoning), text(answer)]);
    }
}

// Source: tests/agent/thinking.rs::a_think_tag_the_answer_writes_is_answer
// (the answer streamed; asked whole, it is in `answers`)
#[tokio::test]
async fn a_think_tag_the_answer_writes_is_answer_streamed() {
    let said = "The parser reads the <think> tag at the start. Everything after is lost.";
    for reasoning in [None, Some("why")] {
        let mut deltas = Vec::new();
        if let Some(reasoning) = reasoning {
            deltas.push(json!({"reasoning_content": reasoning}));
        }
        deltas.push(json!({"content": said}));
        let mut expected: Vec<Piece> = reasoning.map(thinking).into_iter().collect();
        expected.push(text(said));
        assert_eq!(pieces(&deltas).await, expected);
    }
}

// Source: tests/agent/thinking.rs::a_stream_that_only_reasoned_before_it_failed_refuses_no_image
// (what the provider gives: the reasoning is handed on before the failure,
// so the stream was under way; what is done of it there is the host's)
#[tokio::test]
async fn a_stream_that_only_reasoned_before_it_failed_handed_its_reasoning() {
    let reasoned = chunk(&json!({"reasoning_content": "Hm."}));
    let failing = json!({"error": {"message": "invalid image", "code": 400}});
    let body = format!("data: {reasoned}\n\ndata: {failing}\n\n");
    let reply = stream_reply(vec![body.into_bytes()], Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert!(matches!(
        events.as_slice(),
        [StreamEvent::ThinkingDelta(delta)] if delta.thinking == "Hm."
    ));
    let error = failure.unwrap();
    assert_eq!(
        (error.message.as_str(), error.status_code, error.retryable),
        ("invalid image", Some(400), false)
    );
}

// Source: tests/agent/response_schema.rs::entries_with_no_function_are_no_tool_round
// (the answer streamed; asked whole, it is in `answers`)
#[tokio::test]
async fn entries_with_no_function_are_no_tool_round_streamed() {
    let schema = json!({
        "type": "object",
        "properties": {"label": {"type": "string"}},
        "required": ["label"],
        "additionalProperties": false,
    });
    let request = AiRequest {
        response_schema: schema.as_object().cloned(),
        ..hello()
    };
    let reply = events(&[
        chunk(&json!({"tool_calls": [{"index": 0, "id": "c1"}]})),
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
    ]);
    let (_, failure) = streamed(reply, &request).await;
    assert_eq!(
        schema_failure(&failure.unwrap()),
        Some(ResponseSchemaFailure::InvalidJson)
    );
}

// Source: tests/agent/main.rs::the_timeout_bounds_reads_not_the_stream
// The timeout bounds the wait between two reads, not the whole answer, as
// Python's `httpx.Timeout(timeout)` bounds each read. There the events come
// 50 ms apart under a timeout of 150 ms; here most of the timeout is let
// pass before each event, at once, so that the answer lasts three times its
// timeout and nothing waits on real time.
#[tokio::test]
async fn the_timeout_bounds_reads_not_the_stream() {
    let said = ["We ", "open ", "at ", "nine", "."];
    let mut reads = said.map(|text| format!("data: {}\n\n", chunk(&json!({"content": text}))));
    // The answer's end mark comes with its last event.
    if let Some(last) = reads.last_mut() {
        last.push_str("data: [DONE]\n\n");
    }
    let steps = Arc::new(Notify::new());
    let stepped = Reply::Answer {
        status: 200,
        headers: vec![("content-type".to_owned(), "text/event-stream".to_owned())],
        body: Body::Stepped(reads.map(String::into_bytes).to_vec(), Arc::clone(&steps)),
    };
    let fake = Fake::serving(vec![stepped]).await;
    let provider = pointed(elsewhere("local-model"), &fake);
    let timeout = provider.client.timeouts().read();
    let request = hello();
    let mut stream = provider.generate_structured_stream(&request);
    let mut read = String::new();
    for _ in said {
        // The read that waits for the next event begins, and most of its
        // timeout passes before the event is sent.
        assert!(
            poll_once(&mut std::pin::pin!(next(&mut stream)))
                .await
                .is_pending()
        );
        let_pass(timeout.mul_f64(0.6)).await;
        steps.notify_one();
        match next(&mut stream).await {
            Some(Ok(StreamEvent::TextDelta(delta))) => read.push_str(&delta.text),
            other => panic!("a text comes: {other:?}"),
        }
    }
    let (rest, failure) = drained(stream).await;
    assert!(failure.is_none(), "{failure:?}");
    assert!(
        matches!(rest.as_slice(), [StreamEvent::Done(_)]),
        "{rest:?}"
    );
    assert_eq!(read, "We open at nine.");
}

/// A streamed answer that sends `fill` again and again, past `bytes` bytes.
fn endless(fill: &str, bytes: usize) -> Reply {
    let read = fill.repeat((64 * 1024usize).div_ceil(fill.len()));
    let reads = vec![read.clone().into_bytes(); bytes.div_ceil(read.len())];
    stream_reply(reads, Ending::Ends)
}

// Source: tests/agent/main.rs::a_stream_that_never_ends_a_line_is_cut_at_the_line_s_bound
// A stream whose line never ends is cut at the line's bound, 1 MiB, the
// request failing, rather than growing the memory without end.
#[tokio::test]
async fn a_stream_that_never_ends_a_line_is_cut_at_the_line_s_bound() {
    let (events, failure) = streamed(endless("x", TOO_LONG), &hello()).await;
    assert!(events.is_empty(), "{events:?}");
    let error = failure.unwrap();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        (
            "the provider's stream sent a line longer than 1048576 bytes",
            false,
            None
        )
    );
}

// Source: tests/agent/main.rs::a_stream_whose_event_never_ends_is_cut_at_the_event_s_bound
// A stream whose one event never ends, data line after data line, is cut
// at the event's bound, 1 MiB.
#[tokio::test]
async fn a_stream_whose_event_never_ends_is_cut_at_the_event_s_bound() {
    let line = format!("data: {}\n", "x".repeat(1000));
    let (events, failure) = streamed(endless(&line, TOO_LONG), &hello()).await;
    assert!(events.is_empty(), "{events:?}");
    let error = failure.unwrap();
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        (
            "the provider's stream sent an event longer than 1048576 bytes",
            false,
            None
        )
    );
}

// Source: tests/agent/main.rs::a_long_reasoning_streams_past_what_a_whole_answer_may_weigh
// A stream weighs far more than what it says: past the 4 MiB an answer read
// whole may take, a long reasoning is still read to its end.
#[tokio::test]
async fn a_long_reasoning_streams_past_what_a_whole_answer_may_weigh() {
    let mut chunks = vec![chunk(&json!({"reasoning_content": "x"})); 50_000];
    chunks.push(chunk(&json!({"content": "Done."})));
    chunks.extend(ending("stop", 10, 50_001));
    let body = sse_of(&chunks).into_bytes();
    assert!(body.len() > roomkit_providers_client::ANSWER_LIMIT);
    let reads = body.chunks(64 * 1024).map(<[u8]>::to_vec);
    let reply = stream_reply(reads.collect(), Ending::Ends);
    let (events, failure) = streamed(reply, &hello()).await;
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(text_of(&events), "Done.");
    assert_eq!(events.len(), 50_002);
}
