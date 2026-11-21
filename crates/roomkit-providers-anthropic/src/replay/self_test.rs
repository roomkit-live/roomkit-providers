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
use crate::Anthropic;

#[tokio::test]
async fn the_replay_refuses_a_request_that_is_not_python_s() {
    let fixture = fixture();
    let own: [Wrong; 6] = [
        ("another path", "request 0: POST /v1/complete", |r| {
            r.seen[0].target = "/v1/complete".to_owned();
        }),
        ("another key", "x-api-key", |r| {
            let key = "another".to_owned();
            r.seen[0].headers.insert("x-api-key".to_owned(), key);
        }),
        ("another version", "anthropic-version", |r| {
            let version = "2099-01-01".to_owned();
            let headers = &mut r.seen[0].headers;
            headers.insert("anthropic-version".to_owned(), version);
        }),
        ("no accept", "accept", |r| {
            drop(r.seen[0].headers.remove("accept"));
        }),
        (
            "a header more",
            "anthropic-beta is sent, which Python does not send",
            |r| {
                let line = ("anthropic-beta".to_owned(), "tools-2099".to_owned());
                r.seen[0].lines.push(line);
            },
        ),
        ("a header twice", "x-api-key is sent 2 times", |r| {
            let line = r.seen[0].lines.iter().find(|(name, _)| name == "x-api-key");
            let line = line.unwrap().clone();
            r.seen[0].lines.push(line);
        }),
    ];
    let wrongs = [&REQUEST[..], &own[..]].concat();
    refuses::<Anthropic>(&fixture, "plain_answer", &wrongs).await;
    // A listing sends no content type: one that does is refused.
    refuses::<Anthropic>(&fixture, "list_models", &LISTING).await;
}

// The fake serves a stream cut as its case records it, and the replay says
// so when it does not: the reads of a case are joined into one, which the
// provider reads to the same events.
#[tokio::test]
async fn the_replay_refuses_a_stream_served_in_one_piece() {
    let fixture = fixture();
    let streams = [
        ("plain_answer", 6),
        ("stream", 8),
        ("stream_as_text", 8),
        ("stream_cut_before_its_end", 3),
    ];
    refuses_a_stream_served_in_one_piece::<Anthropic>(&fixture, &streams).await;
    // A whole answer is served in no read: one served as a stream is
    // refused too.
    refuses::<Anthropic>(&fixture, "list_models", &SERVED_AS_A_STREAM).await;
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
            let line = ("anthropic-beta".to_owned(), "tools-2099".to_owned());
            r.seen[0].lines.push(line);
        }),
    ];
    let answer = [&NAMED_ANSWER[..], &own[..]].concat();
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, "usage_counts_below_zero", &answer).await;
    let stream = "stream_usage_counts_written_as_text";
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, stream, &NAMED_STREAM).await;
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, "redirect_307", &NAMED_FAILURE).await;

    // A difference less than its entry states is refused as one more is.
    let fewer: [Wrong; 1] = [("the status Python's failure has", OTHERWISE, |r| {
        failure_of(r).status_code = None;
    })];
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, "redirects_past_the_hop_limit", &fewer).await;
    let unbuilt = "base_url_with_a_query";
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, unbuilt, &NAMED_UNBUILT).await;
    // A request the contract does not read is held to why it does not.
    let unread: [Wrong; 2] = [
        ("another reason", NOT_INSTEAD, |r| {
            r.unread = Some("no".to_owned());
        }),
        ("a request all the same", NOT_INSTEAD, |r| {
            r.seen
                .push(roomkit_providers_client::harness::fake::Seen::default());
        }),
    ];
    let role = "turn_whose_role_the_provider_does_not_know";
    held_to_its_entry::<Anthropic>(&fixture, &NAMED, role, &unread).await;
}

#[tokio::test]
async fn the_replay_refuses_waits_that_are_not_python_s() {
    let fixture = fixture();
    refuses::<Anthropic>(&fixture, "retry_that_succeeds", &WAITS).await;
    // A wait the answer's headers ask is exact.
    let asked = "retry_after_the_wait_asked_in_seconds";
    refuses::<Anthropic>(&fixture, asked, &ASKED_WAIT).await;
}

#[tokio::test]
async fn the_replay_refuses_an_outcome_that_is_not_python_s() {
    let fixture = fixture();
    refuses::<Anthropic>(&fixture, "plain_answer", &ANSWER).await;
    refuses::<Anthropic>(&fixture, "answer_200_that_is_a_page", &FAILURE).await;
    refuses::<Anthropic>(&fixture, "response_schema_answer_refused", &SCHEMA).await;
    // What the provider says of its model is compared.
    let vision: [Wrong; 1] = [("another vision", "supports_vision", |r| {
        r.vision = Some(false);
    })];
    refuses::<Anthropic>(
        &fixture,
        "vision_of_a_claude_the_catalogue_does_not_know",
        &vision,
    )
    .await;
}

#[tokio::test]
async fn the_replay_refuses_a_stream_or_a_listing_that_is_not_python_s() {
    let fixture = fixture();
    refuses::<Anthropic>(&fixture, "stream", &STREAM).await;
    // What a stream gave before it failed is compared too.
    refuses::<Anthropic>(&fixture, "stream_cut_before_its_end", &CUT).await;
    // An id the provider mints is compared by its shape, and one call keeps
    // one id.
    let minted: [Wrong; 2] = [
        ("an id of another shape", "the events", |r| {
            for event in events_of(r) {
                if let StreamEvent::ToolCall(call) = event {
                    call.id = "call_clock_1".to_owned();
                }
            }
        }),
        ("two ids for one call", "the events", |r| {
            for event in events_of(r) {
                if let StreamEvent::ToolCall(call) = event {
                    call.id = "call_clock_0123456789ab".to_owned();
                }
            }
        }),
    ];
    refuses::<Anthropic>(&fixture, "stream_tool_call_with_an_empty_id", &minted).await;
    refuses::<Anthropic>(&fixture, "stream_as_text", &TEXT).await;
    refuses::<Anthropic>(&fixture, "list_models", &MODELS).await;
}
