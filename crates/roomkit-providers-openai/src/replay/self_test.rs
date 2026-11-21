//! The replay held to what it says it compares: for each thing, a case is
//! replayed, found to be as Python, then one thing of what was replayed is
//! made wrong, and the replay must say so. So is what it says of its fake,
//! which is made to serve a stream in one piece, and of a named case,
//! which is made to differ or to do otherwise than its entry states.

use roomkit_providers_llm::{
    ProviderError, ProviderErrorOptions, ResponseSchemaError, ResponseSchemaErrorOptions,
    ResponseSchemaFailure, StreamEvent,
};

use super::compare::differences;
use super::fake::{Body, Door, Reply};
use super::named::named;
use super::{Case, Outcome, Replayed, fixture, judged, replay, replay_served, reply_of};

/// One thing made wrong in what a case replayed: its name, and the word of
/// the difference the replay must find.
type Wrong = (&'static str, &'static str, fn(&mut Replayed));

/// The failure a replayed case ended with.
fn failure_of(replayed: &mut Replayed) -> &mut ProviderError {
    match replayed.outcome.as_mut().unwrap() {
        Outcome::Response(Err(error))
        | Outcome::Models(Err(error))
        | Outcome::Events(_, Some(error))
        | Outcome::Text(_, Some(error)) => error,
        other @ (Outcome::Response(Ok(_))
        | Outcome::Models(Ok(_))
        | Outcome::Events(_, None)
        | Outcome::Text(_, None)) => panic!("no failure: {other:?}"),
    }
}

/// The events a replayed stream gave.
fn events_of(replayed: &mut Replayed) -> &mut Vec<StreamEvent> {
    match replayed.outcome.as_mut().unwrap() {
        Outcome::Events(events, _) => events,
        other @ (Outcome::Response(_) | Outcome::Text(..) | Outcome::Models(_)) => {
            panic!("no events: {other:?}")
        }
    }
}

/// A failure of a response schema that says `message` for `reason`.
fn schema_failure(message: &str, reason: ResponseSchemaFailure) -> ProviderError {
    let options = ResponseSchemaErrorOptions {
        provider: "openai".to_owned(),
    };
    ResponseSchemaError::new(message.to_owned(), reason, options).into()
}

/// Replays the case `name`, requires that it is as Python, then requires
/// the replay to refuse each of `wrongs`.
async fn refuses(cases: &[Case], name: &str, wrongs: &[Wrong]) {
    let case = cases.iter().find(|case| case.name == name);
    let case = case.unwrap_or_else(|| panic!("no case {name}"));
    let door = Door::open().await;
    for (what, word, wrong) in wrongs {
        let mut replayed = replay(case, &door).await;
        assert_eq!(differences(case, &replayed), Vec::<String>::new(), "{name}");
        wrong(&mut replayed);
        let found = differences(case, &replayed);
        let said = found.iter().any(|difference| difference.contains(word));
        assert!(said, "{name}, {what}: {found:?}");
    }
}

#[tokio::test]
async fn the_replay_refuses_a_request_that_is_not_python_s() {
    let cases = fixture().cases;
    let wrongs: [Wrong; 12] = [
        ("another body", "its body", |r| {
            r.seen[0].body = b"{}".to_vec();
        }),
        ("no body", "its body", |r| r.seen[0].body.clear()),
        ("another method", "request 0: GET", |r| {
            r.seen[0].method = "GET".to_owned();
        }),
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
        ("a request more", "2 requests sent", |r| {
            r.seen.push(r.seen[0].clone());
        }),
        ("no request", "0 requests sent", |r| r.seen.clear()),
        ("another read timeout", "the read timeout", |r| {
            let timeouts = r.timeouts.as_mut().unwrap();
            timeouts.read *= 4;
        }),
        ("another connect timeout", "the connect timeout", |r| {
            let timeouts = r.timeouts.as_mut().unwrap();
            timeouts.connect = timeouts.read;
        }),
    ];
    refuses(&cases, "plain_answer", &wrongs).await;
    // A listing sends no content type: one that does is refused.
    let listing: [Wrong; 1] = [("a content type", "content-type", |r| {
        let json = "application/json".to_owned();
        r.seen[0].headers.insert("content-type".to_owned(), json);
    })];
    refuses(&cases, "list_models", &listing).await;
}

// The fake serves a stream cut as its case records it, and the replay says
// so when it does not: the reads of a case are joined into one, which the
// provider reads to the same events.
#[tokio::test]
async fn the_replay_refuses_a_stream_served_in_one_piece() {
    let cases = fixture().cases;
    let door = Door::open().await;
    let in_one_piece = |answered: &super::Answered| match reply_of(answered) {
        Reply::Answer {
            status,
            headers,
            body: Body::Reads(reads, ending),
        } => Reply::Answer {
            status,
            headers,
            body: Body::Reads(vec![reads.concat()], ending),
        },
        other @ (Reply::Answer { .. } | Reply::Refused | Reply::Silent) => other,
    };
    for (name, reads) in [
        ("stream", 7),
        ("stream_as_text", 7),
        ("stream_cut_before_its_end", 2),
    ] {
        let case = cases.iter().find(|case| case.name == name).unwrap();
        let as_recorded = replay(case, &door).await;
        assert_eq!(as_recorded.seen[0].reads, reads, "{name}");
        assert_eq!(
            differences(case, &as_recorded),
            Vec::<String>::new(),
            "{name}"
        );
        let joined = replay_served(case, &door, &in_one_piece).await;
        let said =
            format!("request 0: its answer was served in 1 reads, where the case records {reads}");
        assert_eq!(differences(case, &joined), vec![said], "{name}");
    }
    // A whole answer is served in no read: one served as a stream is
    // refused too.
    let wrongs: [Wrong; 1] = [("served as a stream", "served in 1 reads", |r| {
        r.seen[0].reads = 1;
    })];
    refuses(&cases, "plain_answer", &wrongs).await;
}

/// Replays the named case `name`, requires that it differs and does as its
/// entry states, then requires each of `wrongs` to be refused, with `word`
/// in what is said.
async fn held_to_its_entry(cases: &[Case], name: &str, wrongs: &[Wrong]) {
    let case = cases.iter().find(|case| case.name == name);
    let case = case.unwrap_or_else(|| panic!("no case {name}"));
    let named = named();
    let stated = named.get(name);
    assert!(stated.is_some(), "{name} is not named");
    let door = Door::open().await;
    for (what, word, wrong) in wrongs {
        let mut replayed = replay(case, &door).await;
        assert_eq!(judged(case, stated, &replayed), Ok(false), "{name}");
        wrong(&mut replayed);
        let said = judged(case, stated, &replayed).unwrap_err();
        assert!(said.contains(word), "{name}, {what}: {said}");
    }
}

// A named case is held to its entry: it differs from Python in what the
// entry states and in nothing else, and does what the entry states and
// nothing else.
#[tokio::test]
async fn a_named_case_is_held_to_what_its_entry_states() {
    const OTHERWISE: &str = "it differs from Python otherwise than its entry states";
    let cases = fixture().cases;
    let answer: [Wrong; 6] = [
        // What its entry states of what it does.
        ("another text", "not Instead", |r| {
            let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
                panic!("no response");
            };
            response.content.push('!');
        }),
        ("another ending", "not Instead", |r| {
            let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
                panic!("no response");
            };
            response.finish_reason = Some("length".to_owned());
        }),
        ("a count", "not Instead", |r| {
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
        ("another body sent", OTHERWISE, |r| {
            r.seen[0].body = b"{}".to_vec();
        }),
        ("a wait", OTHERWISE, |r| r.waits.push(0.5)),
    ];
    held_to_its_entry(&cases, "usage_counts_below_zero", &answer).await;

    let stream: [Wrong; 3] = [
        ("another text", "not Instead", |r| {
            events_of(r).remove(0);
        }),
        ("served in one piece", OTHERWISE, |r| r.seen[0].reads = 1),
        ("a failure at its end", OTHERWISE, |r| {
            let Some(Outcome::Events(_, failure)) = &mut r.outcome else {
                panic!("no events");
            };
            *failure = Some(ProviderError::new("no", ProviderErrorOptions::default()));
        }),
    ];
    held_to_its_entry(&cases, "stream_event_with_no_data", &stream).await;

    let failure: [Wrong; 4] = [
        ("retried", "not Instead", |r| failure_of(r).retryable = true),
        ("other words", "not Instead", |r| {
            failure_of(r).message.push('!');
        }),
        ("another status", "not Instead", |r| {
            failure_of(r).status_code = Some(308);
        }),
        ("a request more", OTHERWISE, |r| {
            r.seen.push(r.seen[0].clone());
        }),
    ];
    held_to_its_entry(&cases, "redirect", &failure).await;

    // A difference less than its entry states is refused as one more is.
    let fewer: [Wrong; 1] = [("the status Python's failure has", OTHERWISE, |r| {
        failure_of(r).status_code = None;
    })];
    held_to_its_entry(&cases, "redirects_past_the_hop_limit", &fewer).await;
    let unbuilt: [Wrong; 1] = [("other words", "not Instead", |r| {
        r.unbuilt.as_mut().unwrap().message.push('!');
    })];
    held_to_its_entry(
        &cases,
        "base_url_with_a_trailing_slash_and_a_query",
        &unbuilt,
    )
    .await;
}

#[tokio::test]
async fn the_replay_refuses_waits_that_are_not_python_s() {
    let cases = fixture().cases;
    let wrongs: [Wrong; 4] = [
        ("a wait less", "0 waits", |r| r.waits.clear()),
        ("a wait more", "2 waits", |r| r.waits.push(0.5)),
        ("a longer wait", "a wait of", |r| r.waits[0] = 0.51),
        ("a shorter wait", "a wait of", |r| r.waits[0] = 0.37),
    ];
    refuses(&cases, "retry_that_succeeds", &wrongs).await;
    // A wait the answer's headers ask is exact.
    let asked: [Wrong; 1] = [("a wait a little short", "a wait of", |r| r.waits[0] -= 0.01)];
    refuses(&cases, "retry_after_the_wait_asked_in_seconds", &asked).await;
}

#[tokio::test]
async fn the_replay_refuses_an_outcome_that_is_not_python_s() {
    let cases = fixture().cases;
    let answer: [Wrong; 4] = [
        ("another text", "the response", |r| {
            let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
                panic!("no response");
            };
            response.content.push('!');
        }),
        ("a failure", "a failure where Python answers", |r| {
            let error = ProviderError::new("no", ProviderErrorOptions::default());
            r.outcome = Some(Outcome::Response(Err(error)));
        }),
        (
            "another call's result",
            "the events where Python gives none",
            |r| {
                r.outcome = Some(Outcome::Events(Vec::new(), None));
            },
        ),
        ("another vision", "supports_vision", |r| {
            r.vision = Some(false);
        }),
    ];
    refuses(&cases, "plain_answer", &answer).await;

    let failure: [Wrong; 7] = [
        ("another status", "status_code", |r| {
            failure_of(r).status_code = Some(500);
        }),
        ("retried", "retryable", |r| {
            let error = failure_of(r);
            error.retryable = !error.retryable;
        }),
        ("another provider", "provider", |r| {
            failure_of(r).provider = "other".to_owned();
        }),
        ("an overflow", "context_overflow", |r| {
            failure_of(r).context_overflow = Some(true);
        }),
        ("before the request", "before_request", |r| {
            failure_of(r).before_request = true;
        }),
        ("other words", "the failure says", |r| {
            failure_of(r).message.push('!');
        }),
        ("an answer", "an answer where Python fails", |r| {
            r.outcome = Some(Outcome::Models(Ok(Vec::new())));
        }),
    ];
    refuses(&cases, "answer_200_that_is_an_error_object", &failure).await;

    let schema: [Wrong; 2] = [
        ("no schema's failure", "class", |r| {
            let said = failure_of(r).message.clone();
            *failure_of(r) = ProviderError::new(said, ProviderErrorOptions::default());
        }),
        ("another reason", "reason", |r| {
            let said = failure_of(r).message.clone();
            *failure_of(r) = schema_failure(&said, ResponseSchemaFailure::Truncated);
        }),
    ];
    refuses(&cases, "response_schema_refusal", &schema).await;
}

#[tokio::test]
async fn the_replay_refuses_a_stream_or_a_listing_that_is_not_python_s() {
    let cases = fixture().cases;
    let stream: [Wrong; 4] = [
        ("an event less", "the events", |r| drop(events_of(r).pop())),
        ("events in another order", "the events", |r| {
            events_of(r).swap(0, 1);
        }),
        ("an event twice", "the events", |r| {
            let first = events_of(r)[0].clone();
            events_of(r).insert(0, first);
        }),
        (
            "a failure at its end",
            "a failure where Python answers",
            |r| {
                let Some(Outcome::Events(_, failure)) = &mut r.outcome else {
                    panic!("no events");
                };
                *failure = Some(ProviderError::new("no", ProviderErrorOptions::default()));
            },
        ),
    ];
    refuses(&cases, "stream", &stream).await;
    // What a stream gave before it failed is compared too.
    let cut: [Wrong; 2] = [
        ("nothing before the failure", "the events", |r| {
            events_of(r).clear();
        }),
        ("no failure", "an answer where Python fails", |r| {
            let Some(Outcome::Events(_, failure)) = &mut r.outcome else {
                panic!("no events");
            };
            *failure = None;
        }),
    ];
    refuses(&cases, "stream_cut_before_its_end", &cut).await;
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
    refuses(&cases, "stream_tool_call_without_an_id", &minted).await;

    let text: [Wrong; 1] = [("a piece less", "the text", |r| {
        let Some(Outcome::Text(text, _)) = &mut r.outcome else {
            panic!("no text");
        };
        text.pop();
    })];
    refuses(&cases, "stream_as_text", &text).await;
    let models: [Wrong; 2] = [
        ("a model less", "the models", |r| {
            let Some(Outcome::Models(Ok(models))) = &mut r.outcome else {
                panic!("no models");
            };
            models.pop();
        }),
        ("a model not filled", "the models", |r| {
            let Some(Outcome::Models(Ok(models))) = &mut r.outcome else {
                panic!("no models");
            };
            models[0].display_name = None;
        }),
    ];
    refuses(&cases, "list_models", &models).await;
}
