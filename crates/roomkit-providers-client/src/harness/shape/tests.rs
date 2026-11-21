//! The checks of a fixture held to what they refuse: a small sound fixture
//! passes each of them, and each wrong made of it is refused, in the words
//! of the check it trips.

mod sound;
mod wrongs;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use serde_json::Value;

use self::sound::{KEY, LENT_KEY, PROVIDER};
use super::{Case, Failures, Fixture, Shape};

/// What the sound fixture states of itself.
const SHAPE: Shape<'static> = Shape {
    provider: PROVIDER,
    sdk_version: "vendor_version",
    environment: "VENDOR_API_KEY",
    notes: &["environment", "exchanges.response", "questioned"],
    own_members: &["capability"],
    key: KEY,
    request_keys: &[LENT_KEY],
    unread: &["unread"],
    sdk_errors: &["VendorError"],
    sdk_words: &["Connection error."],
    sdk_causes: &["VendorConnectionError"],
    sdk_sentences: &["Expected "],
    raises_an_event_s_data: true,
    longest_wait: 60.0,
};

/// The kinds of case the sound fixture shows.
const KINDS: [&str; 8] = [
    "answer",
    "listing",
    "stream",
    "stream_cut_inside_a_character",
    "failure",
    "retry",
    "redirect",
    "minted",
];

/// What a refused fixture is told.
#[derive(Clone, Copy, Debug)]
enum Says {
    /// These words and no other: an assertion that says its case alone.
    Only(&'static str),
    /// Words that hold these.
    Holding(&'static str),
}

/// One wrong made of the sound fixture: what is wrong, what makes it so,
/// and what the check that refuses it says.
type Wrong = (&'static str, fn(&mut Value), Says);

/// Where the first request of `case` goes: the path of its call under the
/// base URL of its configuration.
fn first(case: &Case) -> String {
    let base_url = case.config["base_url"].as_str().unwrap();
    let path = if case.call == "list_models" {
        "/models"
    } else {
        "/answers"
    };
    format!("{base_url}{path}")
}

/// The timeouts of a case's configuration: a read's, then a connection's.
fn timeouts(config: &Value) -> Option<(Duration, Duration)> {
    let seconds = |name: &str| config[name].as_f64().map(Duration::from_secs_f64);
    seconds("timeout").zip(seconds("connect_timeout"))
}

/// Holds the fixture `text` writes to every check: how many failures it
/// holds, and how many of its reads end inside a character.
fn checked(text: &str) -> (Failures, usize) {
    let fixture = Fixture::read(text, &SHAPE);
    fixture.says_where_it_comes_from(&SHAPE);
    fixture.each_request_reads_as_the_contract_s(&SHAPE);
    fixture.each_result_reads_as_the_contract_s(&SHAPE);
    let failures = fixture.each_failure_says_what_a_replay_reproduces(&SHAPE);
    fixture.the_questioned_cases_are(&["redirected", "to_decide"], &["to_decide"]);
    fixture.each_request_was_sent_where_its_case_expects(first);
    fixture.the_timeouts_are_the_ones_python_sent(timeouts);
    fixture.each_exchange_holds_a_request_and_one_answer(&SHAPE);
    let cut_inside = fixture.each_chunk_reads_as_the_bytes_of_one_read();
    fixture.each_minted_call_id_has_python_s_shape(Some("minted"));
    fixture.a_case_the_sdk_decides_says_so();
    fixture.covers_every_kind_of_case(&KINDS);
    (failures, cut_inside)
}

/// What the checks say of the fixture `text` writes when they refuse it.
fn refused(text: &str) -> Option<String> {
    let said = catch_unwind(AssertUnwindSafe(|| checked(text))).err()?;
    let text = said.downcast_ref::<String>().cloned();
    text.or_else(|| said.downcast_ref::<&str>().map(|said| (*said).to_owned()))
}

// The sound fixture passes every check, and each count is the one it holds.
#[test]
fn a_sound_fixture_passes_every_check() {
    let fixture = sound::fixture();
    let (failures, cut_inside) = checked(&fixture.to_string());
    let expected = Failures {
        all: 7,
        compared: 3,
        of_a_schema: 1,
    };
    assert_eq!((failures, cut_inside), (expected, 2));
    assert_eq!(
        Fixture::read(&fixture.to_string(), &SHAPE).calls("generate"),
        14
    );
}

// Each wrong made of the sound fixture is refused, in the words of the
// check it trips: no check passes a fixture a replay could not rest on.
#[test]
fn each_wrong_made_of_a_sound_fixture_is_refused() {
    let mut missed = Vec::new();
    for (what, wrong, says) in wrongs::ALL.iter().flat_map(|wrongs| wrongs.iter()) {
        let mut fixture = sound::fixture();
        wrong(&mut fixture);
        let said = refused(&fixture.to_string());
        let as_expected = match (&said, says) {
            (Some(said), Says::Only(words)) => said == words,
            (Some(said), Says::Holding(words)) => said.contains(words),
            (None, _) => false,
        };
        if !as_expected {
            missed.push(format!("{what}: {says:?}, and the checks say {said:?}"));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

// The kinds a test lists are each listed once.
#[test]
#[should_panic(expected = "a kind is listed twice")]
fn a_kind_listed_twice_is_refused() {
    let fixture = Fixture::read(&sound::fixture().to_string(), &SHAPE);
    fixture.covers_every_kind_of_case(&["answer", "answer"]);
}
