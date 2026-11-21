//! The replay held to what it says it compares: for each thing, a case is
//! replayed, found to be as Python, then one thing of what was replayed is
//! made wrong, and the replay must say so. So is what it says of its fake,
//! which is made to serve a stream in one piece, and of a named case,
//! which is made to differ or to do otherwise than its entry states.
//!
//! The harness of `roomkit-providers-client` holds what is made wrong
//! whatever the wire; the wrongs of this provider's wire are written here.

use roomkit_providers_client::harness::Outcome;
use roomkit_providers_client::harness::self_test::{
    ANSWER, ASKED_WAIT, CUT, FAILURE, LISTING, MODELS, NAMED_ANSWER, NAMED_FAILURE, NAMED_STREAM,
    NAMED_UNBUILT, NOT_INSTEAD, OTHERWISE, REQUEST, SCHEMA, SERVED_AS_A_STREAM, STREAM, TEXT,
    WAITS, Wrong, events_of, failure_of, held_to_its_entry, refuses,
    refuses_a_stream_served_in_one_piece,
};
use roomkit_providers_llm::StreamEvent;

use super::fixture;
use super::named::NAMED;
use crate::OpenAi;

#[tokio::test]
async fn the_replay_refuses_a_request_that_is_not_python_s() {
    let fixture = fixture();
    let own: [Wrong; 5] = [
        ("another path", "request 0: POST /v1/chat", |r| {
            r.seen[0].target = "/v1/chat".to_owned();
        }),
        ("another key", "authorization", |r| {
            let bearer = "Bearer another".to_owned();
            r.seen[0].headers.insert("authorization".to_owned(), bearer);
        }),
        ("no accept", "accept", |r| {
            drop(r.seen[0].headers.remove("accept"));
        }),
        (
            "a header more",
            "x-api-key is sent, which Python does not send",
            |r| {
                let line = ("x-api-key".to_owned(), "placeholder-not-a-key".to_owned());
                r.seen[0].lines.push(line);
            },
        ),
        ("a header twice", "authorization is sent 2 times", |r| {
            let line = r.seen[0]
                .lines
                .iter()
                .find(|(name, _)| name == "authorization");
            let line = line.unwrap().clone();
            r.seen[0].lines.push(line);
        }),
    ];
    let wrongs = [&REQUEST[..], &own[..]].concat();
    refuses::<OpenAi>(&fixture, "plain_answer", &wrongs).await;
    // A listing sends no content type: one that does is refused.
    refuses::<OpenAi>(&fixture, "list_models", &LISTING).await;
}

// The fake serves a stream cut as its case records it, and the replay says
// so when it does not: the reads of a case are joined into one, which the
// provider reads to the same events.
#[tokio::test]
async fn the_replay_refuses_a_stream_served_in_one_piece() {
    let fixture = fixture();
    let streams = [
        ("stream", 7),
        ("stream_as_text", 7),
        ("stream_cut_before_its_end", 2),
    ];
    refuses_a_stream_served_in_one_piece::<OpenAi>(&fixture, &streams).await;
    // A whole answer is served in no read: one served as a stream is
    // refused too.
    refuses::<OpenAi>(&fixture, "plain_answer", &SERVED_AS_A_STREAM).await;
}

// A named case is held to its entry: it differs from Python in what the
// entry states and in nothing else, and does what the entry states and
// nothing else.
#[tokio::test]
async fn a_named_case_is_held_to_what_its_entry_states() {
    let fixture = fixture();
    let own: [Wrong; 2] = [
        // What its entry states of what it does.
        ("a count", NOT_INSTEAD, |r| {
            let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
                panic!("no response");
            };
            response.usage.insert("input_tokens".to_owned(), 12.into());
        }),
        // A difference its entry does not state, the provider doing what
        // the entry states.
        ("a header more", OTHERWISE, |r| {
            let line = ("x-api-key".to_owned(), "placeholder-not-a-key".to_owned());
            r.seen[0].lines.push(line);
        }),
    ];
    let answer = [&NAMED_ANSWER[..], &own[..]].concat();
    held_to_its_entry::<OpenAi>(&fixture, &NAMED, "usage_counts_below_zero", &answer).await;
    held_to_its_entry::<OpenAi>(&fixture, &NAMED, "stream_event_with_no_data", &NAMED_STREAM).await;
    held_to_its_entry::<OpenAi>(&fixture, &NAMED, "redirect", &NAMED_FAILURE).await;

    // A difference less than its entry states is refused as one more is.
    let fewer: [Wrong; 1] = [("the status Python's failure has", OTHERWISE, |r| {
        failure_of(r).status_code = None;
    })];
    held_to_its_entry::<OpenAi>(&fixture, &NAMED, "redirects_past_the_hop_limit", &fewer).await;
    let unbuilt = "base_url_with_a_trailing_slash_and_a_query";
    held_to_its_entry::<OpenAi>(&fixture, &NAMED, unbuilt, &NAMED_UNBUILT).await;
}

#[tokio::test]
async fn the_replay_refuses_waits_that_are_not_python_s() {
    let fixture = fixture();
    refuses::<OpenAi>(&fixture, "retry_that_succeeds", &WAITS).await;
    // A wait the answer's headers ask is exact.
    let asked = "retry_after_the_wait_asked_in_seconds";
    refuses::<OpenAi>(&fixture, asked, &ASKED_WAIT).await;
}

#[tokio::test]
async fn the_replay_refuses_an_outcome_that_is_not_python_s() {
    let fixture = fixture();
    refuses::<OpenAi>(&fixture, "plain_answer", &ANSWER).await;
    let failing = "answer_200_that_is_an_error_object";
    refuses::<OpenAi>(&fixture, failing, &FAILURE).await;
    refuses::<OpenAi>(&fixture, "response_schema_refusal", &SCHEMA).await;
}

#[tokio::test]
async fn the_replay_refuses_a_stream_or_a_listing_that_is_not_python_s() {
    let fixture = fixture();
    refuses::<OpenAi>(&fixture, "stream", &STREAM).await;
    // What a stream gave before it failed is compared too.
    refuses::<OpenAi>(&fixture, "stream_cut_before_its_end", &CUT).await;
    // An id the provider mints is compared by its shape, and one call keeps
    // one id.
    let minted: [Wrong; 2] = [
        ("an id of another shape", "the events", |r| {
            for event in events_of(r) {
                if let StreamEvent::ToolCall(call) = event {
                    call.id = "call_lookup_order_1".to_owned();
                }
            }
        }),
        ("two ids for one call", "the events", |r| {
            for event in events_of(r) {
                if let StreamEvent::ToolCall(call) = event {
                    call.id = "call_lookup_order_0123456789ab".to_owned();
                }
            }
        }),
    ];
    refuses::<OpenAi>(&fixture, "stream_tool_call_without_an_id", &minted).await;
    refuses::<OpenAi>(&fixture, "stream_as_text", &TEXT).await;
    refuses::<OpenAi>(&fixture, "list_models", &MODELS).await;
}
