//! Text from outside (RFC §6.4) against what `RoomKit`'s Python makes of it:
//! `fixtures/python_outside_text.json`, written by
//! `scripts/generate_python_outside_text.py`.
//!
//! Each fenced case holds a tag and a text with the block Python's
//! `roomkit._text.fence` sets the text apart in; each identifier, a name and
//! a fallback with what Python's `roomkit._text.identifier` gives.

use roomkit_providers_llm::{fence, identifier};
use serde::Deserialize;

const FIXTURE: &str = include_str!("fixtures/python_outside_text.json");

#[derive(Deserialize)]
struct Fixture {
    fenced: Vec<Block>,
    identifiers: Vec<Named>,
}

#[derive(Deserialize)]
struct Block {
    tag: String,
    text: String,
    fenced: String,
}

#[derive(Deserialize)]
struct Named {
    value: String,
    fallback: String,
    identifier: String,
}

#[test]
fn a_text_is_fenced_as_python_fences_it() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture.fenced.len(), 275, "the fixture lost cases");
    let kept_as_written = fixture
        .fenced
        .iter()
        .filter(|case| case.fenced == format!("<{0}>\n{1}\n</{0}>", case.tag, case.text))
        .count();
    assert_eq!(kept_as_written, 13, "the fixture lost texts with no tag");
    let missed: Vec<(&str, &str, String)> = fixture
        .fenced
        .iter()
        .filter_map(|case| {
            let fenced = fence(&case.tag, &case.text);
            (fenced != case.fenced).then_some((case.tag.as_str(), case.text.as_str(), fenced))
        })
        .collect();
    assert_eq!(missed, Vec::<(&str, &str, String)>::new());
}

#[test]
fn a_name_is_the_identifier_python_gives() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture.identifiers.len(), 35, "the fixture lost names");
    let missed: Vec<(&str, String)> = fixture
        .identifiers
        .iter()
        .filter_map(|case| {
            let given = identifier(&case.value, &case.fallback);
            (given != case.identifier).then_some((case.value.as_str(), given))
        })
        .collect();
    assert_eq!(missed, Vec::<(&str, String)>::new());
}
