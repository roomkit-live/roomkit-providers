//! The provider against the wire cases captured on `RoomKit`'s Python
//! provider (`tests/fixtures/python_messages_cases.json`): each case is
//! replayed by the harness of `roomkit-providers-client` through a local
//! fake of the endpoint, which serves a stream read by read, and what the
//! provider sends and gives is compared with Python's: the requests, the
//! waits between two tries, and the response, the events, the text or the
//! models, a failure by the rule its case gives. The fake says in how many
//! reads it wrote each answer, which is compared with the reads the case
//! records.
//!
//! No case is left out silently. A case the provider answers otherwise than
//! Python is named in [`named`], with the reason, how its replay differs
//! from Python's and what the provider does instead: the replay must differ
//! in that and in nothing else, and the provider must do exactly that. A
//! named case that starts to replay as Python fails the test.

mod named;
mod self_test;

use std::net::SocketAddr;

use roomkit_providers_client::Client;
pub(crate) use roomkit_providers_client::harness::fake;
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
pub(crate) use roomkit_providers_client::harness::{drained, let_pass, poll_once};
use roomkit_providers_llm::{AiProvider, ProviderError};
use serde::Deserialize;
use serde_json::Value;

use self::named::NAMED;
use crate::{Anthropic, AnthropicConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_messages_cases.json");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

impl Replays for Anthropic {
    const KEY_HEADER: &'static str = "x-api-key";

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: AnthropicConfig = serde_json::from_value(config.clone()).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }
}

/// What Python's provider read of its model's catalogue entry.
#[derive(Deserialize)]
struct Catalogue {
    model: String,
    known: bool,
    tags: Vec<String>,
    reasoning_floor: Option<String>,
}

fn fixture() -> Fixture {
    Fixture::read(FIXTURE)
}

/// Replays every case of the fixture whose call is `call`: how many
/// replayed as Python, and how many are named and differ and do as their
/// entry states. Fails for a case that is neither, and for a named case
/// that replays as Python.
async fn replayed_or_named_for(call: &str) -> (usize, usize) {
    let fixture = fixture();
    assert!((fixture.clock() - CLOCK).abs() < f64::EPSILON);
    replayed_or_named::<Anthropic>(&fixture, &NAMED, call).await
}

/// Every `generate` case of the fixture: replayed as Python, or named with
/// what the provider does instead.
#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate").await, (208, 34));
}

/// Every `generate_structured_stream` case, the same way: the events in
/// their order, what was given before a failure, and the failure.
#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named_for("generate_structured_stream").await,
        (80, 20)
    );
}

/// Every `generate_stream` case: the text as it came, and the failure that
/// ended it.
#[tokio::test]
async fn every_text_stream_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate_stream").await, (5, 1));
}

/// Every `list_models` case: the models, or what the provider does of a
/// failure Python's `list_models` raises as its SDK's or as a raw exception.
#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("list_models").await, (7, 9));
}

// The seam a replay outside this crate drives a provider through does what
// this replay does of its own: the requests go to the fake, a date is read
// against the clock given, and the waits are kept, not taken.
#[cfg(feature = "replay")]
#[tokio::test]
async fn the_seam_of_a_replay_outside_the_crate_does_as_this_one() {
    use self::fake::Fake;

    let fixture = fixture();
    let dated = fixture.case("retry_after_the_wait_asked_as_a_date");
    let config: AnthropicConfig = serde_json::from_value(dated.config.clone()).unwrap();
    let fake = Fake::serving(dated.replies()).await;
    let provider = Anthropic::new(config.clone()).unwrap();
    let (provider, waits) = provider.replaying(fake.address, CLOCK).unwrap();
    let request = dated.asked();
    let response = provider.generate(&request).await.unwrap();
    let expected = dated.expected_response().unwrap().to_string();
    assert_eq!(serde_json::to_string(&response).unwrap(), expected);
    assert_eq!(fake.seen().len(), 2);
    let [wait] = waits.taken()[..] else {
        panic!("one wait: {:?}", waits.taken());
    };
    assert!((wait - 7.0).abs() < 1e-6, "the wait the date asks: {wait}");
    // Read against the system's clock, the date is long past: the wait is
    // the backoff's, within a quarter below half a second.
    let fake = Fake::serving(dated.replies()).await;
    let provider = Anthropic::new(config.clone()).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    let (provider, waits) = provider
        .replaying(fake.address, now.unwrap().as_secs_f64())
        .unwrap();
    provider.generate(&request).await.unwrap();
    let [wait] = waits.taken()[..] else {
        panic!("one wait: {:?}", waits.taken());
    };
    assert!((0.375..=0.5).contains(&wait), "{wait}");
    // Nothing is sent off the machine.
    let elsewhere = "192.0.2.1:80".parse().unwrap();
    let refused = Anthropic::new(config).unwrap().replaying(elsewhere, CLOCK);
    let refused = refused.unwrap_err();
    assert!(refused.before_request && !refused.retryable);
    assert_eq!(
        refused.message,
        "the provider cannot be built: a replay sends to a loopback address only"
    );
}

/// The catalogue of the crate says of each model what the cases rest on:
/// a case that changed because Python's catalogue did shows here.
#[test]
fn the_catalogue_says_what_the_cases_rest_on() {
    let read = [
        crate::models::DEFERRED_TOOLS,
        crate::models::EFFORT_REFUSED,
        crate::models::EFFORT_NO_XHIGH,
        crate::models::EFFORT_UP_TO_HIGH,
        crate::models::THINKING_OFF_DISABLED,
        crate::models::THINKING_OFF_BETWEEN_TOOLS,
        crate::models::THINKING_OFF_EFFORT_HIGH,
    ];
    let mut models = 0;
    for case in fixture().cases {
        let facts = case.member("catalogue").unwrap();
        let facts: Catalogue = serde_json::from_value(facts.clone()).unwrap();
        assert_eq!(facts.model, case.config["model"], "{}", case.name);
        let entry = crate::models::entry(&facts.model);
        let tags = read
            .into_iter()
            .filter(|tag| crate::models::tagged(entry, tag));
        assert_eq!(entry.is_some(), facts.known, "{}", case.name);
        assert_eq!(tags.collect::<Vec<_>>(), facts.tags, "{}", case.name);
        let floor = crate::models::reasoning_floor(entry);
        assert_eq!(floor, facts.reasoning_floor.as_deref(), "{}", case.name);
        models += 1;
    }
    assert!(models >= 364, "{models} cases");
}

/// What the provider of each case answers of itself is what Python's
/// answers: whether it holds a tool declared and unseen, which the harness
/// does not compare, beside what is true of every configuration.
#[test]
fn a_provider_answers_of_itself_what_python_s_answers() {
    let (mut holds, mut built) = (0, 0);
    for case in fixture().cases {
        let Ok(provider) = <Anthropic as Replays>::built(&case.config) else {
            continue;
        };
        built += 1;
        let deferred = case.member("supports_deferred_tools").unwrap();
        assert_eq!(
            Value::from(provider.supports_deferred_tools()),
            *deferred,
            "{}",
            case.name
        );
        holds += usize::from(provider.supports_deferred_tools());
        assert_eq!(provider.name(), "AnthropicAIProvider");
        assert_eq!(provider.label(), "anthropic");
        assert_eq!(provider.model_name(), case.config["model"]);
        assert!(provider.supports_streaming() && provider.supports_structured_streaming());
        assert!(
            provider.supports_response_schema() && provider.supports_response_schema_with_tools()
        );
    }
    // The two the provider does not build: a base URL Python's client and
    // this one read otherwise.
    assert_eq!(built, 362);
    assert!((270..built).contains(&holds), "{holds} hold a tool unseen");
}
