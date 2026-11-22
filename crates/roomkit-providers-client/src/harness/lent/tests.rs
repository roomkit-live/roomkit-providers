//! The check of a lent key held to what it says: a provider that leaves a
//! generation with its request passes it, and one that leaves without it,
//! or whose failure shows nothing of the refusal, does not.

use roomkit_ai::AiContext;
use roomkit_providers_python_forms::Form;
use serde_json::{Value, json};

use super::kept_out;
use crate::harness::fake::Door;
use crate::harness::least::{
    ASKING_NOTHING, KEY, Least, SAYING_NOTHING, WITH_ITS_REQUEST, WITHOUT_ITS_REQUEST,
};
use crate::harness::named::Named;
use crate::harness::{Fixture, replayed_or_named};

/// A fixture of one case for each generation, whose provider leaves as
/// `leaves` says.
fn fixture(leaves: &str) -> Fixture {
    let request = AiContext::default().written();
    let case = |call: &str| {
        json!({
            "name": call,
            "call": call,
            "config": {"api_key": KEY, "leaves": leaves},
            "request": request,
            "exchanges": [],
            "expected": {},
        })
    };
    let calls = ["generate", "generate_structured_stream", "generate_stream"];
    let cases: Vec<Value> = calls.into_iter().map(case).collect();
    Fixture::read(&json!({"clock": "2026-01-01T00:00:00Z", "cases": cases}).to_string())
}

/// What the check says of each generation of a provider that leaves as
/// `leaves` says.
async fn checked(leaves: &str) -> Vec<Result<bool, String>> {
    let (fixture, door) = (fixture(leaves), Door::open().await);
    let mut said = Vec::new();
    for case in &fixture.cases {
        said.push(kept_out::<Least>(case, &door).await);
    }
    said
}

// Each of the three generations is held: its failure says what the 401
// says, without the key lent to the request.
#[tokio::test]
async fn a_generation_that_leaves_with_its_request_keeps_the_lent_key_out() {
    assert_eq!(checked(WITH_ITS_REQUEST).await, vec![Ok(true); 3]);
}

// A provider that leaves a generation without its request repeats the key
// the 401 repeats: each of the three generations is refused.
#[tokio::test]
async fn a_generation_that_leaves_without_its_request_is_refused() {
    for wrong in checked(WITHOUT_ITS_REQUEST).await {
        let wrong = wrong.unwrap_err();
        let said = "the failure repeats the key lent to the request: ";
        assert!(wrong.starts_with(said), "{wrong}");
    }
}

// A failure that says nothing of the 401 shows nothing of the key: it is
// refused, where a request that was never sent is only not counted.
#[tokio::test]
async fn a_failure_that_shows_nothing_of_the_refusal_holds_nothing() {
    for wrong in checked(SAYING_NOTHING).await {
        let wrong = wrong.unwrap_err();
        let said = "the failure says nothing of the 401 that repeats the key lent to the request";
        assert!(wrong.starts_with(said), "{wrong}");
    }
    assert_eq!(checked(ASKING_NOTHING).await, vec![Ok(false); 3]);
}

// The replay of a fixture asks the check of each case: a provider that
// leaves without its request fails it there, for that reason among others.
#[tokio::test]
#[should_panic(expected = "the failure repeats the key lent to the request")]
async fn the_replay_of_a_fixture_holds_its_provider_to_the_check() {
    let named = Named {
        questioned_otherwise: &[],
        only_python: &[],
        by_its_own_rule: &[],
    };
    let fixture = fixture(WITHOUT_ITS_REQUEST);
    replayed_or_named::<Least>(&fixture, &named, "generate").await;
}
