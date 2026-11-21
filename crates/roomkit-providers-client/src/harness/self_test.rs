//! The replay held to what it says it compares: for each thing, a case is
//! replayed, found to be as Python, then one thing of what was replayed is
//! made wrong, and the replay must say so. So is what it says of its fake,
//! which is made to serve a stream in one piece, and of a named case,
//! which is made to differ or to do otherwise than its entry states.
//!
//! A provider crate names the cases of its fixture to make wrong, and adds
//! to the lists here, which hold whatever its wire, the wrongs of its own:
//! another path, another key, a header it does not send.

use roomkit_providers_llm::{
    ProviderError, ProviderErrorOptions, ResponseSchemaError, ResponseSchemaErrorOptions,
    ResponseSchemaFailure, StreamEvent,
};

use super::compare::differences;
use super::fake::{Body, Door, Reply};
use super::named::Named;
use super::{
    Answered, Fixture, Outcome, Replayed, Replays, judged, replay, replay_served, reply_of,
};
use crate::Timeouts;

/// One thing made wrong in what a case replayed: its name, a word of the
/// difference the replay must find, and what makes it wrong.
pub type Wrong = (&'static str, &'static str, fn(&mut Replayed));

/// The failure a replayed case ended with.
pub fn failure_of(replayed: &mut Replayed) -> &mut ProviderError {
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
pub fn events_of(replayed: &mut Replayed) -> &mut Vec<StreamEvent> {
    match replayed.outcome.as_mut().unwrap() {
        Outcome::Events(events, _) => events,
        other @ (Outcome::Response(_) | Outcome::Text(..) | Outcome::Models(_)) => {
            panic!("no events: {other:?}")
        }
    }
}

/// The failure of a response schema, for `reason`, that says what `error`
/// says, of the same provider.
fn schema_failure(error: &ProviderError, reason: ResponseSchemaFailure) -> ProviderError {
    let options = ResponseSchemaErrorOptions {
        provider: error.provider.clone(),
    };
    ResponseSchemaError::new(error.message.clone(), reason, options).into()
}

/// A failure that says no more than `no`.
fn no() -> ProviderError {
    ProviderError::new("no", ProviderErrorOptions::default())
}

/// Replays the case `name` of `fixture`, requires that it is as Python,
/// then requires the replay to refuse each of `wrongs`, with its word in
/// what is said.
pub async fn refuses<P: Replays>(fixture: &Fixture, name: &str, wrongs: &[Wrong]) {
    let case = fixture.case(name);
    let door = Door::open().await;
    for (what, word, wrong) in wrongs {
        let mut replayed = replay::<P>(fixture, case, &door).await;
        assert_eq!(differences(case, &replayed), Vec::<String>::new(), "{name}");
        wrong(&mut replayed);
        let found = differences(case, &replayed);
        let said = found.iter().any(|difference| difference.contains(word));
        assert!(said, "{name}, {what}: {found:?}");
    }
}

/// Replays each of `streams`, a case of `fixture` by its name with the
/// reads its answer is recorded in, as it is recorded, then with its reads
/// joined into one, which the provider reads to the same events: the replay
/// must say that the answer was served in one read.
pub async fn refuses_a_stream_served_in_one_piece<P: Replays>(
    fixture: &Fixture,
    streams: &[(&str, usize)],
) {
    let door = Door::open().await;
    let in_one_piece = |answered: &Answered| match reply_of(answered) {
        Reply::Answer {
            status,
            headers,
            body: Body::Reads(reads, ending),
        } => Reply::Answer {
            status,
            headers,
            body: Body::Reads(vec![reads.concat()], ending),
        },
        other @ (Reply::Answer { .. } | Reply::Refused | Reply::Silent | Reply::Tunnel(_)) => other,
    };
    for (name, reads) in streams {
        let case = fixture.case(name);
        let as_recorded = replay::<P>(fixture, case, &door).await;
        assert_eq!(as_recorded.seen[0].reads, *reads, "{name}");
        assert_eq!(
            differences(case, &as_recorded),
            Vec::<String>::new(),
            "{name}"
        );
        let joined = replay_served::<P>(fixture, case, &door, &in_one_piece).await;
        let said =
            format!("request 0: its answer was served in 1 reads, where the case records {reads}");
        assert_eq!(differences(case, &joined), vec![said], "{name}");
    }
}

/// Replays the case `name` of `fixture`, one of `named`, requires that it
/// differs and does as its entry states, then requires each of `wrongs` to
/// be refused, with its word in what is said.
pub async fn held_to_its_entry<P: Replays>(
    fixture: &Fixture,
    named: &Named,
    name: &str,
    wrongs: &[Wrong],
) {
    let case = fixture.case(name);
    let named = named.stated();
    let stated = named.get(name);
    assert!(stated.is_some(), "{name} is not named");
    let door = Door::open().await;
    for (what, word, wrong) in wrongs {
        let mut replayed = replay::<P>(fixture, case, &door).await;
        assert_eq!(judged::<P>(case, stated, &replayed), Ok(false), "{name}");
        wrong(&mut replayed);
        let said = judged::<P>(case, stated, &replayed).unwrap_err();
        assert!(said.contains(word), "{name}, {what}: {said}");
    }
}

/// What a replay says of a named case that differs from Python otherwise
/// than its entry states.
pub const OTHERWISE: &str = "it differs from Python otherwise than its entry states";

/// What a replay says of a named case that does otherwise than its entry
/// states.
pub const NOT_INSTEAD: &str = "not Instead";

/// A request that is not Python's, for a case that posts one request and is
/// answered, and whose configuration names no `User-Agent`: its body, its
/// method, how many are sent, the timeouts of the client that sent it, and
/// the name the client gives itself.
pub const REQUEST: [Wrong; 9] = [
    ("another body", "its body", |r| {
        r.seen[0].body = b"{}".to_vec();
    }),
    ("no body", "its body", |r| r.seen[0].body.clear()),
    ("another method", "request 0: GET", |r| {
        r.seen[0].method = String::from("GET");
    }),
    ("a request more", "2 requests sent", |r| {
        r.seen.push(r.seen[0].clone());
    }),
    ("no request", "0 requests sent", |r| r.seen.clear()),
    ("another read timeout", "the read timeout", |r| {
        let built = r.timeouts.unwrap();
        r.timeouts = Timeouts::new(built.read() * 4, built.connect()).ok();
    }),
    ("another connect timeout", "the connect timeout", |r| {
        let built = r.timeouts.unwrap();
        r.timeouts = Timeouts::new(built.read(), built.read()).ok();
    }),
    ("no user agent", "user-agent is None", |r| {
        drop(r.seen[0].headers.remove("user-agent"));
        r.seen[0].lines.retain(|(name, _)| name != "user-agent");
    }),
    ("another user agent", "user-agent is Some", |r| {
        let (name, other) = ("user-agent".to_owned(), "example/1.0".to_owned());
        r.seen[0].headers.insert(name, other);
    }),
];

/// For a case that lists the models: a listing sends no content type, and
/// one that does is refused.
pub const LISTING: [Wrong; 1] = [("a content type", "content-type", |r| {
    let json = "application/json".to_owned();
    r.seen[0].headers.insert("content-type".to_owned(), json);
})];

/// For a case answered whole: a whole answer is served in no read, and one
/// served as a stream is refused.
pub const SERVED_AS_A_STREAM: [Wrong; 1] = [("served as a stream", "served in 1 reads", |r| {
    r.seen[0].reads = 1;
})];

/// Waits that are not Python's, for a case tried again once after its first
/// backoff, half a second or up to a quarter less.
pub const WAITS: [Wrong; 4] = [
    ("a wait less", "0 waits", |r| r.waits.clear()),
    ("a wait more", "2 waits", |r| r.waits.push(0.5)),
    ("a longer wait", "a wait of", |r| r.waits[0] = 0.51),
    ("a shorter wait", "a wait of", |r| r.waits[0] = 0.37),
];

/// For a case tried again after the wait its answer's headers ask: such a
/// wait is exact.
pub const ASKED_WAIT: [Wrong; 1] = [("a wait a little short", "a wait of", |r| r.waits[0] -= 0.01)];

/// An outcome that is not Python's, for a `generate` case that answers.
pub const ANSWER: [Wrong; 4] = [
    ("another text", "the response", |r| {
        let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
            panic!("no response");
        };
        response.content.push('!');
    }),
    ("a failure", "a failure where Python answers", |r| {
        r.outcome = Some(Outcome::Response(Err(no())));
    }),
    (
        "another call's result",
        "the events where Python gives none",
        |r| {
            r.outcome = Some(Outcome::Events(Vec::new(), None));
        },
    ),
    ("another vision", "supports_vision", |r| {
        r.vision = r.vision.map(|vision| !vision);
    }),
];

/// A failure that is not Python's, for a case that fails with a
/// `ProviderError` whose every attribute a replay reproduces.
pub const FAILURE: [Wrong; 7] = [
    ("another status", "status_code", |r| {
        let error = failure_of(r);
        error.status_code = Some(if error.status_code == Some(500) {
            502
        } else {
            500
        });
    }),
    ("retried", "retryable", |r| {
        let error = failure_of(r);
        error.retryable = !error.retryable;
    }),
    ("another provider", "provider", |r| {
        failure_of(r).provider = String::from("other");
    }),
    ("an overflow", "context_overflow", |r| {
        let error = failure_of(r);
        error.context_overflow = Some(error.context_overflow != Some(true));
    }),
    ("before the request", "before_request", |r| {
        let error = failure_of(r);
        error.before_request = !error.before_request;
    }),
    ("other words", "the failure says", |r| {
        failure_of(r).message.push('!');
    }),
    ("an answer", "an answer where Python fails", |r| {
        r.outcome = Some(Outcome::Models(Ok(Vec::new())));
    }),
];

/// For a case that fails as a response schema's refusal: a failure that is
/// no schema's, and one of another reason.
pub const SCHEMA: [Wrong; 2] = [
    ("no schema's failure", "class", |r| {
        let said = failure_of(r).message.clone();
        *failure_of(r) = ProviderError::new(said, ProviderErrorOptions::default());
    }),
    ("another reason", "reason", |r| {
        let truncated = schema_failure(failure_of(r), ResponseSchemaFailure::Truncated);
        *failure_of(r) = truncated;
    }),
];

/// A stream that is not Python's, for a `generate_structured_stream` case
/// that gives two events or more and ends without a failure.
pub const STREAM: [Wrong; 4] = [
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
            *failure = Some(no());
        },
    ),
];

/// For a stream that gives events, then fails: what a stream gave before it
/// failed is compared too.
pub const CUT: [Wrong; 2] = [
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

/// For a `generate_stream` case that gives text.
pub const TEXT: [Wrong; 1] = [("a piece less", "the text", |r| {
    let Some(Outcome::Text(text, _)) = &mut r.outcome else {
        panic!("no text");
    };
    text.pop();
})];

/// For a `list_models` case whose first model the catalogue fills with a
/// name to display.
pub const MODELS: [Wrong; 2] = [
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

/// For a named case whose entry states a whole answer that stops for
/// `stop`: what the entry states of what it does, then a difference the
/// entry does not state, the provider doing what the entry states.
pub const NAMED_ANSWER: [Wrong; 4] = [
    ("another text", NOT_INSTEAD, |r| {
        let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
            panic!("no response");
        };
        response.content.push('!');
    }),
    ("another ending", NOT_INSTEAD, |r| {
        let Some(Outcome::Response(Ok(response))) = &mut r.outcome else {
            panic!("no response");
        };
        response.finish_reason = Some("length".to_owned());
    }),
    ("another body sent", OTHERWISE, |r| {
        r.seen[0].body = b"{}".to_vec();
    }),
    ("a wait", OTHERWISE, |r| r.waits.push(0.5)),
];

/// For a named case whose entry states a stream that ends without a
/// failure.
pub const NAMED_STREAM: [Wrong; 3] = [
    ("another text", NOT_INSTEAD, |r| {
        events_of(r).remove(0);
    }),
    ("served in one piece", OTHERWISE, |r| r.seen[0].reads = 1),
    ("a failure at its end", OTHERWISE, |r| {
        let Some(Outcome::Events(_, failure)) = &mut r.outcome else {
            panic!("no events");
        };
        *failure = Some(no());
    }),
];

/// For a named case whose entry states a failure after one request.
pub const NAMED_FAILURE: [Wrong; 4] = [
    ("retried or not", NOT_INSTEAD, |r| {
        let error = failure_of(r);
        error.retryable = !error.retryable;
    }),
    ("other words", NOT_INSTEAD, |r| {
        failure_of(r).message.push('!');
    }),
    ("another status", NOT_INSTEAD, |r| {
        let error = failure_of(r);
        error.status_code = Some(if error.status_code == Some(500) {
            502
        } else {
            500
        });
    }),
    ("a request more", OTHERWISE, |r| {
        r.seen.push(r.seen[0].clone());
    }),
];

/// For a named case whose entry states a provider that is not built.
pub const NAMED_UNBUILT: [Wrong; 1] = [("other words", NOT_INSTEAD, |r| {
    r.unbuilt.as_mut().unwrap().message.push('!');
})];
