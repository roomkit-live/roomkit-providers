//! The provider against the wire cases captured on `RoomKit`'s Python
//! Mistral provider (`tests/fixtures/python_mistral_cases.json`, written by
//! `scripts/generate_mistral_cases.py` from `scripts/mistral_cases.py`).
//! Each case is replayed by the harness of `roomkit-providers-client`
//! through a local fake of the endpoint, and what the provider sends and
//! gives is compared with Python's. A case the provider answers otherwise
//! is named, with what it does instead.

use std::net::SocketAddr;

use roomkit_ai::{FinishReason, ProviderError};
use roomkit_providers_client::Client;
use roomkit_providers_client::harness::named::{Does, Entry, Instead, Named, fails};
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
use serde_json::Value;

use crate::{Mistral, MistralConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_mistral_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

impl Replays for Mistral {
    const KEY_HEADER: &'static str = "authorization";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: MistralConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }
}

/// The cases whose failure no provider reproduces: the mistralai SDK's own,
/// or a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[Entry] = &[(
    "models_listing_failed",
    "Python's list_models wraps no error: the mistralai SDK's is raised as it is. The \
     provider fails as it does for a generation.",
    &["a SDKError no provider reproduces"],
    fails(
        Some(401),
        false,
        "the provider answered HTTP 401: Unauthorized",
    ),
)];

/// The cases the provider answers otherwise by a rule of its own. What it
/// does instead.
const BY_ITS_OWN_RULE: &[Entry] = &[(
    "empty_key",
    "A key goes in its header as it is written, an empty one too, as the request's key of \
     Anthropic's provider goes (KEY_AS_WRITTEN): the provider sends `Bearer `, which a server, \
     the fake among them, reads as `Bearer`, white space around a header's value being no \
     part of it. Python's HTTP client refuses that value on a wire (`Illegal header value`): \
     it sends nothing, and fails as a lost connection, worth a retry.",
    &[r#"request 0: authorization is Some("Bearer")"#],
    Instead {
        requests: 1,
        does: Does::Answers(
            "Hello there.",
            FinishReason::Stop,
            r#"{"input_tokens":12,"output_tokens":5}"#,
        ),
    },
)];

const NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: ONLY_PYTHON,
    by_its_own_rule: BY_ITS_OWN_RULE,
};

fn fixture() -> Fixture {
    Fixture::read(FIXTURE)
}

async fn replayed_or_named_for(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert!((fixture.clock() - CLOCK).abs() < f64::EPSILON);
    replayed_or_named::<Mistral>(&fixture, &NAMED, call).await
}

#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate").await, (78, 1));
}

#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named_for("generate_structured_stream").await,
        (29, 0)
    );
}

#[tokio::test]
async fn every_text_stream_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate_stream").await, (3, 0));
}

#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("list_models").await, (3, 1));
}

/// What the data file of the named cases says of itself.
const NOTES: &str = "The cases of python_mistral_cases.json the provider answers otherwise than \
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
