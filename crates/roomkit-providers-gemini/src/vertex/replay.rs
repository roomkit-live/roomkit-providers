//! The provider against the wire cases captured on `RoomKit`'s Python
//! Gemini provider on Vertex AI (`tests/fixtures/python_vertex_cases.json`,
//! written by `scripts/generate_vertex_cases.py` from
//! `scripts/vertex_cases.py`). Each case is replayed by the harness of
//! `roomkit-providers-client` through a local fake of Google's token
//! endpoint and of Vertex AI, and what the provider sends and gives is
//! compared with Python's: the assertion of the key to its signature, the
//! fixture's clock and test key being Python's. A case the provider answers
//! otherwise is named, with what it does instead.
//!
//! The fake stands in for every host: the host a location is asked on is
//! checked against the fixture's URLs apart, as the provider builds them.

use std::net::SocketAddr;
use std::time::Duration;

use base64::{Engine, engine::general_purpose::STANDARD};
use roomkit_ai::ProviderError;
use roomkit_providers_client::harness::named::{Does, Entry, Instead, Named, Says};
use roomkit_providers_client::harness::{Fixture, Replays, replayed_or_named};
use roomkit_providers_client::{Client, Pace, Timeouts};
use serde_json::Value;

use super::{GeminiVertex, GeminiVertexConfig};

const FIXTURE: &str = include_str!("../../tests/fixtures/python_vertex_cases.json");

/// The test key of the fixtures, PKCS#8.
const TEST_KEY: &[u8] = include_bytes!("../../tests/fixtures/test-only-rsa-pkcs8.der");

/// The fixture's clock, in seconds since 1970: 2026-01-01T00:00:00Z.
const CLOCK: f64 = 1_767_225_600.0;

/// How long google-auth's token requests wait to connect and to read.
const TOKEN_TIMEOUT: Duration = Duration::from_secs(120);

/// The PEM of the test key, as a key file writes it.
fn test_key_pem() -> String {
    let der = STANDARD.encode(TEST_KEY);
    let lines: Vec<&str> = der
        .as_bytes()
        .chunks(64)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect();
    format!(
        "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        lines.join("\n")
    )
}

/// `config` with its key written whole: the members the fixture gives under
/// `KEYED`, and the test key, as the capture builds it.
fn keyed(config: &Value) -> Value {
    let mut config = config.clone();
    let key = config
        .get("service_account_json")
        .and_then(|key| key.get("KEYED"))
        .cloned();
    if let Some(Value::Object(mut key)) = key {
        key.insert("private_key".to_owned(), test_key_pem().into());
        config["service_account_json"] = Value::Object(key).to_string().into();
    }
    config
}

impl Replays for GeminiVertex {
    const KEY_HEADER: &'static str = "authorization";
    // The first request asks a token of the identity, whose failures stay in
    // closed words.
    const SAYS_REFUSALS: bool = false;

    fn built(config: &Value) -> Result<Self, ProviderError> {
        let config: GeminiVertexConfig = serde_json::from_value(keyed(config)).unwrap();
        Self::new(config)
    }

    fn client(&mut self) -> &mut Client {
        &mut self.inner.client
    }

    fn point_at(&mut self, fake: SocketAddr) {
        Self::point_at(self, fake);
    }

    fn paced(&mut self, pace: Pace) {
        self.inner.client.set_pace(pace.clone());
        *self.pace.lock().unwrap() = pace;
    }

    // google-auth asks its tokens on a client of its own.
    fn timeouts_by_path(&self) -> Vec<(&'static str, Timeouts)> {
        let timeouts = Timeouts::new(TOKEN_TIMEOUT, TOKEN_TIMEOUT).unwrap();
        vec![("/token", timeouts)]
    }
}

/// Why a token the identity could not get reads otherwise than Python's.
const TOKEN_REFUSED: &str = "a token the identity could not get fails before the request to \
     Vertex AI, as every failure of a host's token source fails (before_request), in the \
     client's own words, never the endpoint's: google-auth's error says nothing of it";

/// What the provider says of a token it could not get.
const UNAVAILABLE: &str = "provider credentials are unavailable; check the connection and account";

const BY_ITS_OWN_RULE: &[Entry] = &[
    (
        "key_refused",
        TOKEN_REFUSED,
        &["the failure's before_request"],
        Instead {
            requests: 1,
            does: Does::FailsBefore(Says::Exactly(UNAVAILABLE)),
        },
    ),
    (
        "key_endpoint_down",
        TOKEN_REFUSED,
        &["the failure's before_request"],
        Instead {
            requests: 3,
            does: Does::FailsBefore(Says::Exactly(UNAVAILABLE)),
        },
    ),
    (
        "key_endpoint_unreachable",
        TOKEN_REFUSED,
        &["the failure's before_request"],
        Instead {
            requests: 1,
            does: Does::FailsBefore(Says::Exactly(UNAVAILABLE)),
        },
    ),
    (
        "key_token_page",
        TOKEN_REFUSED,
        &["the failure's before_request"],
        Instead {
            requests: 1,
            does: Does::FailsBefore(Says::Exactly(UNAVAILABLE)),
        },
    ),
];

/// The cases whose failure no provider reproduces: the google-genai SDK's
/// own. What the provider does instead.
const ONLY_PYTHON: &[Entry] = &[(
    "models_listing_failed",
    "Python's list_models wraps no error: the google-genai SDK's is raised as it is. The \
     provider fails as it does for a generation.",
    &["a ClientError no provider reproduces"],
    Instead {
        requests: 2,
        does: Does::Fails(Some(403), false, None, Says::Exactly("Permission denied.")),
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
    replayed_or_named::<GeminiVertex>(&fixture, &NAMED, call).await
}

#[tokio::test]
async fn every_generate_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("generate").await, (15, 4));
}

#[tokio::test]
async fn every_structured_stream_case_replays_as_python_or_is_named() {
    assert_eq!(
        replayed_or_named_for("generate_structured_stream").await,
        (1, 0)
    );
}

#[tokio::test]
async fn every_listing_case_replays_as_python_or_is_named() {
    assert_eq!(replayed_or_named_for("list_models").await, (1, 1));
}

/// The host of each case's first request to Vertex AI, which the fake does
/// not see: as the provider builds it, Python's.
#[test]
fn each_location_is_asked_on_python_s_host() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let config: GeminiVertexConfig = serde_json::from_value(keyed(&case["config"])).unwrap();
        let provider = GeminiVertex::new(config).unwrap();
        let exchanges = case["exchanges"].as_array().unwrap();
        let asked = exchanges
            .iter()
            .map(|exchange| &exchange.get("request").unwrap()["url"]);
        let vertex = asked
            .filter_map(Value::as_str)
            .find(|url| url.contains("aiplatform"));
        let Some(vertex) = vertex else { continue };
        let built = if case["call"] == "list_models" {
            &provider.inner.models
        } else {
            &provider.inner.generate
        };
        assert_eq!(built.as_str(), vertex, "{}", case["name"]);
    }
}

/// What the data file of the named cases says of itself.
const NOTES: &str = "The cases of python_vertex_cases.json the provider answers otherwise than \
                     Python, as src/vertex/replay.rs states them, for a replay outside the \
                     crate. 'differs' lists the differences from Python a replay finds, each by \
                     the words it starts with; 'requests' is how many requests the provider \
                     sends; 'does' is what the call gives. A test of the crate keeps this file \
                     equal to the source: run that test with VERTEX_NAMED_CASES_OUT naming the \
                     file to rewrite it.";

#[test]
fn the_named_cases_file_is_what_this_module_states() {
    let stated = NAMED.as_data(NOTES);
    if let Some(out) = std::env::var_os("VERTEX_NAMED_CASES_OUT") {
        std::fs::write(out, stated).unwrap();
        return;
    }
    let held = include_str!("../../tests/fixtures/vertex_named_cases.json");
    assert!(
        held == stated,
        "rewrite tests/fixtures/vertex_named_cases.json (VERTEX_NAMED_CASES_OUT), and commit the file"
    );
}

/// Labels built by hand are held to Google's rules when the provider is
/// built, as Python's configuration holds them when it is.
#[test]
fn labels_google_refuses_refuse_the_provider() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let config: GeminiVertexConfig =
        serde_json::from_value(keyed(&fixture["cases"][0]["config"])).unwrap();
    for labels in [vec![("Tenant", "acme")], vec![("tenant", "Acme")]] {
        let labels = labels
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()));
        let config = GeminiVertexConfig {
            labels: Some(labels.collect()),
            ..config.clone()
        };
        let refused = GeminiVertex::new(config).unwrap_err();
        assert!(refused.before_request, "{refused:?}");
        assert!(refused.message.contains("labels"), "{refused:?}");
    }
}
