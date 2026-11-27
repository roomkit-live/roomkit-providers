//! The provider against the wire cases captured on `RoomKit`'s Python
//! Gemini provider (`tests/fixtures/python_gemini_cases.json`, written by
//! `scripts/generate_gemini_cases.py` from `scripts/gemini_cases.py`). Each
//! case is replayed by the harness of `roomkit-providers-client` through a
//! local fake of the endpoint, and what the provider sends and gives is
//! compared with Python's. A case the provider answers otherwise is named,
//! with what it does instead.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::named::{Entry, Named, fails};
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
use serde::Deserialize;
use serde_json::Value;

use crate::{Gemini, GeminiConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_gemini_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

impl Replays for Gemini {
    const KEY_HEADER: &'static str = "x-goog-api-key";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: GeminiConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }
}

/// The cases whose failure no provider reproduces: the google-genai SDK's
/// own, or a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[Entry] = &[(
    "models_listing_failed",
    "Python's list_models wraps no error: the google-genai SDK's is raised as it is. The \
     provider fails as it does for a generation.",
    &["a ClientError no provider reproduces"],
    fails(Some(401), false, "API key not valid."),
)];

const NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: ONLY_PYTHON,
    by_its_own_rule: &[],
};

#[derive(Deserialize)]
struct Catalogue {
    model: String,
    known: bool,
    tags: Vec<String>,
}

fn fixture() -> Fixture {
    Fixture::read(FIXTURE)
}

async fn replayed_or_named_for(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert!((fixture.clock() - CLOCK).abs() < f64::EPSILON);
    replayed_or_named::<Gemini>(&fixture, &NAMED, call).await
}

#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate").await, (110, 0));
}

#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named_for("generate_structured_stream").await,
        (6, 0)
    );
}

#[tokio::test]
async fn every_text_stream_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate_stream").await, (1, 0));
}

#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("list_models").await, (4, 1));
}

#[test]
fn the_catalogue_says_what_the_cases_rest_on() {
    let read = [
        crate::models::TAKES_LEVELS,
        crate::models::TAKES_MINIMAL,
        crate::models::CANNOT_STOP,
    ];
    let mut cases = 0;
    for case in fixture().cases {
        let facts = case.member("catalogue").unwrap();
        let facts: Catalogue = serde_json::from_value(facts.clone()).unwrap();
        let entry = crate::models::entry(&facts.model);
        let capabilities = crate::models::capabilities(entry);
        let tags: Vec<&str> = read
            .into_iter()
            .filter(|tag| capabilities.iter().any(|capability| capability == tag))
            .collect();
        assert_eq!(entry.is_some(), facts.known, "{}", case.name);
        assert_eq!(tags, facts.tags, "{}", case.name);
        cases += 1;
    }
    assert!(cases >= 122, "{cases} cases");
}

/// What the data file of the named cases says of itself.
const NOTES: &str = "The cases of python_gemini_cases.json the provider answers otherwise than \
                     Python, as src/replay/mod.rs states them, for a replay outside the crate. \
                     'differs' lists the differences from Python a replay finds, each by the \
                     words it starts with; 'requests' is how many requests the provider sends; \
                     'does' is what the call gives. A test of the crate keeps this file equal to \
                     the source: run that test with NAMED_CASES_OUT naming the file to rewrite \
                     it.";

#[test]
fn the_named_cases_file_is_what_this_module_states() {
    let stated = NAMED.as_data(NOTES);
    if let Some(out) = std::env::var_os("NAMED_CASES_OUT") {
        std::fs::write(out, stated).unwrap();
        return;
    }
    let held = include_str!("../../tests/fixtures/named_cases.json");
    assert!(
        held == stated,
        "rewrite tests/fixtures/named_cases.json (NAMED_CASES_OUT), and commit the file"
    );
}

#[test]
fn each_named_case_is_one_the_fixture_sets_apart() {
    NAMED.set_apart(&fixture());
}
