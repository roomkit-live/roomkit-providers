//! The wrongs made of the sound fixture, one at least for each thing a
//! check refuses: each changes one thing of the fixture, and names what the
//! check that refuses it says. A wrong trips one assertion alone, so that
//! an assertion that goes lets its wrong through.

mod exchanges;
mod read;
mod results;

use serde_json::{Map, Value, json};

use super::Wrong;

/// The case named `name`.
pub(super) fn case<'f>(fixture: &'f mut Value, name: &str) -> &'f mut Value {
    let cases = fixture["cases"].as_array_mut().unwrap();
    let found = cases.iter_mut().find(|case| case["name"] == name);
    found.unwrap()
}

/// The request Python sent in the exchange at `at` of the case `name`.
pub(super) fn sent<'f>(fixture: &'f mut Value, name: &str, at: usize) -> &'f mut Value {
    &mut case(fixture, name)["exchanges"][at]["request"]
}

/// What the fake answered in the exchange at `at` of the case `name`.
pub(super) fn answer<'f>(fixture: &'f mut Value, name: &str, at: usize) -> &'f mut Value {
    &mut case(fixture, name)["exchanges"][at]["response"]
}

/// The error Python raised in the case `name`.
pub(super) fn error<'f>(fixture: &'f mut Value, name: &str) -> &'f mut Value {
    &mut case(fixture, name)["expected"]["error"]
}

/// Takes `member` out of `object`.
pub(super) fn without(object: &mut Value, member: &str) {
    drop(object.as_object_mut().unwrap().remove(member));
}

/// Takes the last item out of `list`.
pub(super) fn without_its_last(list: &mut Value) {
    drop(list.as_array_mut().unwrap().pop());
}

/// Writes the members of `object` in the other order.
pub(super) fn backwards(object: &mut Value) {
    let members = object.as_object().unwrap().iter().rev();
    let members = members.map(|(name, held)| (name.clone(), held.clone()));
    *object = members.collect::<Map<String, Value>>().into();
}

/// A wait of `seconds` the SDK took.
pub(super) fn wait(seconds: f64) -> Value {
    json!({"seconds": seconds, "from": "the backoff"})
}

/// Every wrong, by the check that refuses it.
pub(super) const ALL: [&[Wrong]; 12] = [
    read::STRICTLY_READ,
    read::WHERE_IT_COMES_FROM,
    read::REQUESTS,
    results::RESULTS,
    results::ERRORS,
    results::COMPARED,
    results::QUESTIONED,
    exchanges::SENT,
    exchanges::EXCHANGES,
    exchanges::WAITS,
    exchanges::CHUNKS,
    exchanges::THE_REST,
];
