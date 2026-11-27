//! The provider against the wire cases captured on `RoomKit`'s Python
//! PolarGrid provider (`tests/fixtures/python_polargrid_cases.json`, written
//! by `scripts/generate_polargrid_cases.py` from
//! `scripts/polargrid_cases.py`). Each case is replayed by the harness of
//! `roomkit-providers-client` through a local fake of the autorouter and the
//! edge, and what the provider sends and gives is compared with Python's. A
//! case the provider answers otherwise is named, with what it does instead.

use std::net::SocketAddr;

use roomkit_ai::ProviderError;
use roomkit_providers_client::harness::named::Named;
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
use roomkit_providers_client::{Client, Timeouts};
use serde_json::Value;

use crate::{PolarGrid, PolarGridConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_polargrid_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

impl Replays for PolarGrid {
    const KEY_HEADER: &'static str = "authorization";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: PolarGridConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }

    // The autorouter is asked through a client of its own.
    fn timeouts_by_path(&self) -> Vec<(&'static str, Timeouts)> {
        vec![("/v1/route", self.router_client.timeouts())]
    }
}

const NAMED: Named = Named {
    questioned_otherwise: &[],
    only_python: &[],
    by_its_own_rule: &[],
};

fn fixture() -> Fixture {
    Fixture::read(FIXTURE)
}

async fn replayed_or_named_for(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert!((fixture.clock() - CLOCK).abs() < f64::EPSILON);
    replayed_or_named::<PolarGrid>(&fixture, &NAMED, call).await
}

#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate").await, (68, 0));
}

#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named_for("generate_structured_stream").await,
        (23, 0)
    );
}

#[tokio::test]
async fn every_text_stream_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate_stream").await, (2, 0));
}

#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("list_models").await, (5, 0));
}

/// What the data file of the named cases says of itself.
const NOTES: &str = "The cases of python_polargrid_cases.json the provider answers otherwise \
                     than Python, as src/replay/mod.rs states them, for a replay outside the \
                     crate. 'differs' lists the differences from Python a replay finds, each by \
                     the words it starts with; 'requests' is how many requests the provider \
                     sends; 'does' is what the call gives. A test of the crate keeps this file \
                     equal to the source: run that test with NAMED_CASES_OUT naming the file to \
                     rewrite it.";

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
