//! The last step of a replay held to what it says: a case is counted when
//! it replays as Python, or when it is named and differs and does as its
//! entry states. Any other case fails the replay, in words that say which
//! case and why: one that differs from Python and is not named, a named
//! one that replays as Python, and a named one that differs or does
//! otherwise than its entry states.

use roomkit_ai::{AiContext, AiResponse};
use roomkit_providers_python_forms::Form;
use serde_json::{Value, json};

use super::least::{KEY, Least, URL, WITH_ITS_REQUEST};
use super::named::{Entry, Instead, Named, fails};
use super::self_test::{NOT_INSTEAD, OTHERWISE};
use super::{Fixture, replayed_or_named};

/// The case the provider of a test answers as Python did.
const AS_PYTHON: &str = "answered_as_python";

/// The case the provider of a test fails where Python answered.
const REFUSED: &str = "refused_where_python_answered";

/// What the provider says of the refusal of [`REFUSED`].
const SAID: &str = "the provider answered HTTP 500: Internal Server Error";

/// A `generate` case named `name`, whose one request the fake answers with
/// `status` and an empty object, and that Python answered with an empty
/// response.
fn case(name: &str, status: u16) -> Value {
    let sent = json!({
        "method": "POST",
        "url": URL,
        "headers": {"accept": "*/*", "content-type": "application/json"},
        "timeouts": {"connect": 5.0, "read": 30.0},
        "body": {},
    });
    let answered = json!({
        "status": status,
        "headers": {"content-type": "application/json"},
        "json": {},
    });
    json!({
        "name": name,
        "call": "generate",
        "config": {"api_key": KEY, "leaves": WITH_ITS_REQUEST},
        "request": AiContext::default().written(),
        "exchanges": [{"request": sent, "response": answered}],
        "expected": {"response": AiResponse::new("").written()},
    })
}

/// The fixture of three cases: two the provider answers as Python, one it
/// fails where Python answered.
fn fixture() -> Fixture {
    let cases = [
        case(AS_PYTHON, 200),
        case("answered_once_more", 200),
        case(REFUSED, 500),
    ];
    Fixture::read(&json!({"clock": "2026-01-01T00:00:00Z", "cases": cases}).to_string())
}

/// The named cases that are `entries`, each by its own rule.
const fn naming(entries: &'static [Entry]) -> Named {
    Named {
        questioned_otherwise: &[],
        only_python: &[],
        by_its_own_rule: entries,
    }
}

/// An entry for `name`, which differs from Python as `differs` says and
/// does `instead`.
const fn entry(name: &'static str, differs: &'static [&'static str], instead: Instead) -> Entry {
    (name, "The fake refuses the request.", differs, instead)
}

/// How the replay of [`REFUSED`] differs from Python's, by the words the
/// difference starts with.
const DIFFERS: &[&str] = &["a failure where Python answers"];

/// What the provider does of [`REFUSED`].
const INSTEAD: Instead = fails(Some(500), true, SAID);

/// No case named.
const NO_CASE: Named = naming(&[]);

/// [`REFUSED`] named, as it differs and as the provider does.
const AS_IT_IS: Named = naming(&[entry(REFUSED, DIFFERS, INSTEAD)]);

/// What the replay of `call` over the fixture says when it fails, the
/// cases named as `named` says.
async fn failed(named: &'static Named, call: &'static str) -> String {
    let replayed = async move { replayed_or_named::<Least>(&fixture(), named, call).await };
    // The replay runs as a task of its own, whose panic is what it says.
    let tasks = tokio::task::LocalSet::new();
    let failed = tasks.run_until(tasks.spawn_local(replayed)).await;
    let said = failed.unwrap_err().into_panic();
    let text = said.downcast_ref::<String>().cloned();
    text.unwrap_or_else(|| (*said.downcast_ref::<&str>().unwrap()).to_owned())
}

// A case that replays as Python and a named case that differs and does as
// its entry states are each counted, and the replay passes. A call that
// lists the models lends no key: it passes with no case at all.
#[tokio::test]
async fn a_case_as_python_and_a_named_one_as_its_entry_states_are_counted() {
    let counts = replayed_or_named::<Least>(&fixture(), &AS_IT_IS, "generate").await;
    assert_eq!(counts, (2, 1));
    let listing = replayed_or_named::<Least>(&fixture(), &AS_IT_IS, "list_models").await;
    assert_eq!(listing, (0, 0));
}

// A case that differs from Python and that no entry names fails the replay,
// which says the case and its difference, and nothing of the case that
// replays as Python.
#[tokio::test]
async fn a_case_that_differs_from_python_and_is_not_named_fails_the_replay() {
    let said = failed(&NO_CASE, "generate").await;
    let expected = format!("{REFUSED}: a failure where Python answers: ");
    assert!(said.starts_with(&expected), "{said}");
    assert!(!said.contains(AS_PYTHON), "{said}");
}

// A named case that replays as Python fails the replay: its entry states a
// difference the provider no longer has.
#[tokio::test]
async fn a_named_case_that_replays_as_python_fails_the_replay() {
    const BOTH: Named = naming(&[
        entry(AS_PYTHON, DIFFERS, INSTEAD),
        entry(REFUSED, DIFFERS, INSTEAD),
    ]);
    let said = failed(&BOTH, "generate").await;
    assert_eq!(
        said,
        format!("{AS_PYTHON}: named, and it replays as Python")
    );
}

// A named case fails the replay when it differs from Python otherwise than
// its entry states: in another thing, or in one thing more.
#[tokio::test]
async fn a_named_case_that_differs_otherwise_than_its_entry_states_fails_the_replay() {
    const ANOTHER: Named = naming(&[entry(REFUSED, &["the response"], INSTEAD)]);
    const ONE_MORE: Named = naming(&[entry(
        REFUSED,
        &["a failure where Python answers", "the response"],
        INSTEAD,
    )]);
    for named in [&ANOTHER, &ONE_MORE] {
        let said = failed(named, "generate").await;
        let expected = format!("{REFUSED}: {OTHERWISE}");
        assert!(said.starts_with(&expected), "{said}");
    }
}

// A named case fails the replay when the provider does otherwise than its
// entry states: another status, other words, a failure that is final,
// another count of requests.
#[tokio::test]
async fn a_named_case_that_does_otherwise_than_its_entry_states_fails_the_replay() {
    const ANOTHER_STATUS: Named = naming(&[entry(REFUSED, DIFFERS, fails(Some(503), true, SAID))]);
    const OTHER_WORDS: Named = naming(&[entry(
        REFUSED,
        DIFFERS,
        fails(Some(500), true, "the provider answered HTTP 500"),
    )]);
    const FINAL: Named = naming(&[entry(REFUSED, DIFFERS, fails(Some(500), false, SAID))]);
    const TWO_REQUESTS: Named = naming(&[entry(
        REFUSED,
        DIFFERS,
        Instead {
            requests: 2,
            ..INSTEAD
        },
    )]);
    for named in [&ANOTHER_STATUS, &OTHER_WORDS, &FINAL, &TWO_REQUESTS] {
        let said = failed(named, "generate").await;
        let expected = format!("{REFUSED}: {NOT_INSTEAD}");
        assert!(said.starts_with(&expected), "{said}");
    }
}

// A generation none of whose cases shows the key lent to a request kept out
// of a failure fails the replay: a fixture that holds no case of it shows
// nothing.
#[tokio::test]
async fn a_generation_no_case_shows_the_lent_key_kept_out_of_fails_the_replay() {
    let said = failed(&NO_CASE, "generate_stream").await;
    assert_eq!(
        said,
        "generate_stream: no case shows the key lent to a request kept out of a failure"
    );
}
