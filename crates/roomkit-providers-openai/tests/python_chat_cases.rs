//! The wire cases captured on `RoomKit`'s Python `OpenAI` provider, read
//! with the contract's types and this crate's configuration:
//! `tests/fixtures/python_chat_cases.json`, written by
//! `scripts/generate_openai_chat_cases.py` (`make python-openai-chat-cases`).
//!
//! Each case holds a configuration, a request, the HTTP exchanges Python had
//! with a fake of the endpoint, and what Python returned. The provider that
//! replays them is not written yet: this test holds the table to its shape,
//! reads every part of it, and requires every kind of case.

use std::collections::{BTreeMap, BTreeSet};

use roomkit_providers_core::{
    AiRequest, AiResponse, ProviderError, ProviderErrorOptions, ResponseSchemaError,
    ResponseSchemaErrorOptions, ResponseSchemaFailure, StreamEvent,
};
use roomkit_providers_openai::OpenAiConfig;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_chat_cases.json");

/// The key of every configuration, and the key a request carries.
const KEY: &str = "placeholder-not-a-key";
const REQUEST_KEY: &str = "placeholder-not-a-key-either";

#[derive(Deserialize)]
struct Fixture {
    roomkit_version: String,
    openai_version: String,
    httpx2_version: String,
    python_version: String,
    clock: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    kinds: Vec<String>,
    call: String,
    #[serde(rename = "note")]
    _note: Option<String>,
    #[serde(rename = "sdk")]
    _sdk: Option<String>,
    config: Value,
    request: Value,
    request_api_key: Option<String>,
    exchanges: Vec<Exchange>,
    expected: Expected,
    #[serde(default)]
    minted_call_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    request: Sent,
    response: Answered,
    wait: Option<Wait>,
}

/// What Python sent.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sent {
    method: String,
    path: String,
    headers: BTreeMap<String, Option<String>>,
    timeouts: BTreeMap<String, f64>,
    body: Value,
}

/// What the fake answered: a status and headers with one body, or a failure
/// in place of an answer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answered {
    status: Option<u16>,
    headers: Option<BTreeMap<String, String>>,
    json: Option<Value>,
    text: Option<String>,
    chunks: Option<Vec<String>>,
    then: Option<String>,
    failure: Option<String>,
}

/// The wait the SDK took before its next try.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wait {
    seconds: f64,
    from: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    response: Option<Value>,
    text: Option<Vec<String>>,
    events: Option<Vec<Value>>,
    error: Option<Failure>,
}

/// The error Python raised: its class, what it says, and the attributes of
/// a `ProviderError`, which another class does not have.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    class: String,
    message: String,
    text: String,
    raised_from: Option<String>,
    retryable: Option<bool>,
    provider: Option<String>,
    status_code: Option<u16>,
    context_overflow: Option<bool>,
    before_request: Option<bool>,
    reason: Option<String>,
}

/// What a fixture must show of the provider: each kind, in at least one
/// case. A regenerated fixture that loses one fails here.
const KINDS: [&str; 64] = [
    // A request and its answer.
    "plain",
    "system_prompt",
    "turns",
    "image_data_uri",
    "image_url",
    // Tools.
    "tools",
    "tool_call",
    "tool_call_streamed",
    "tool_calls_several",
    "tool_call_unreadable",
    "tool_round_replayed",
    "tool_result_parts",
    "minted_call_id",
    // Reasoning, the temperature and the token bound.
    "reasoning_effort",
    "no_temperature",
    "max_completion_tokens",
    "thinking_field",
    "thinking_read",
    // A response schema.
    "response_schema",
    "response_schema_unsupported",
    "response_schema_with_tools",
    "response_schema_mismatch",
    "refusal",
    "truncated",
    // The configuration on the wire.
    "extra_body",
    "default_headers",
    "timeouts",
    "redirect",
    "request_key",
    "stream",
    "stream_usage_on",
    "stream_usage_off",
    "stream_cut_anywhere",
    "usage_cached",
    "usage_reasoning",
    // Endings.
    "finish_stop",
    "finish_length",
    "finish_content_filter",
    "finish_tool_calls",
    "empty_answer",
    // Failures.
    "before_request",
    "status_400",
    "status_401",
    "status_403",
    "status_404",
    "status_408",
    "status_409",
    "status_413",
    "status_422",
    "status_429",
    "status_500",
    "status_502",
    "status_503",
    "retry_after_seconds",
    "retry_after_date",
    "context_overflow",
    "not_json",
    "error_object",
    "stream_cut",
    "stream_error_event",
    "connection_refused",
    "timeout",
    // Retries of the SDK.
    "retry_succeeds",
    "retries_exhausted",
];

#[expect(
    clippy::unwrap_used,
    reason = "a test's helper: a fixture that does not read fails the test"
)]
fn fixture() -> Fixture {
    serde_json::from_str(FIXTURE).unwrap()
}

/// Reads `value` as `T` and requires that `T` writes it back unchanged, its
/// members in the same order.
#[expect(
    clippy::panic,
    reason = "a test's helper: a case that does not read fails the test, naming it"
)]
fn read_back<T: DeserializeOwned + Serialize>(value: &Value, label: &str) -> T {
    let read = T::deserialize(value).unwrap_or_else(|error| panic!("{label}: {error}"));
    let written = serde_json::to_string(&read).ok();
    assert_eq!(written, Some(value.to_string()), "{label}");
    read
}

#[test]
fn the_fixture_names_the_versions_it_was_written_from() {
    let fixture = fixture();
    for version in [
        &fixture.roomkit_version,
        &fixture.openai_version,
        &fixture.httpx2_version,
        &fixture.python_version,
    ] {
        assert!(
            version.starts_with(|c: char| c.is_ascii_digit()),
            "{version}"
        );
    }
    assert_eq!(fixture.clock, "2026-01-01T00:00:00Z");
}

#[test]
fn each_configuration_reads_as_an_openai_configuration() {
    for case in fixture().cases {
        let config: OpenAiConfig = serde_json::from_value(case.config.clone())
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(config.model, case.config["model"], "{}", case.name);
        let key = case.config["api_key"].as_str().unwrap();
        assert!(key.is_empty() || key == KEY, "{}", case.name);
        assert_eq!(config.api_key, key, "{}", case.name);
    }
}

/// What the configuration says of the token bound and of the temperature is
/// what Python sent.
#[test]
fn the_switches_of_a_configuration_are_the_ones_python_sent() {
    for case in fixture().cases {
        let config: OpenAiConfig = serde_json::from_value(case.config.clone()).unwrap();
        let written = serde_json::to_value(&config).unwrap();
        for exchange in &case.exchanges {
            let body = exchange.request.body.as_object().unwrap();
            let bound = ["max_tokens", "max_completion_tokens"].map(|name| body.contains_key(name));
            if bound != [false, false] {
                let modern = written["use_max_completion_tokens"] == true;
                assert_eq!(bound, [!modern, modern], "{}", case.name);
            }
            assert_eq!(
                written["supports_custom_temperature"],
                body.contains_key("temperature"),
                "{}",
                case.name
            );
        }
    }
}

/// The timeouts of a configuration are the ones each request was sent
/// with: its own for a read, a write and the pool, the other to connect.
#[test]
fn the_timeouts_of_a_configuration_are_the_ones_python_sent() {
    for case in fixture().cases {
        let Ok(config) = serde_json::from_value::<OpenAiConfig>(case.config.clone()) else {
            continue;
        };
        let (whole, connect) = (config.timeout, config.connect_timeout);
        for exchange in &case.exchanges {
            let sent: BTreeMap<&str, f64> = exchange
                .request
                .timeouts
                .iter()
                .map(|(name, seconds)| (name.as_str(), *seconds))
                .collect();
            let expected = BTreeMap::from([
                ("connect", connect.as_secs_f64()),
                ("pool", whole.as_secs_f64()),
                ("read", whole.as_secs_f64()),
                ("write", whole.as_secs_f64()),
            ]);
            assert_eq!(sent, expected, "{}", case.name);
        }
    }
}

#[test]
fn each_request_reads_as_the_contract_s_request_and_writes_back_unchanged() {
    for case in fixture().cases {
        let request: AiRequest = read_back(&case.request, &case.name);
        // A request's own key is written masked, and plain beside it.
        let masked = case.request["metadata"].get("api_key").is_some();
        assert_eq!(case.request_api_key.is_some(), masked, "{}", case.name);
        if let Some(key) = &case.request_api_key {
            assert_eq!(key, REQUEST_KEY, "{}", case.name);
            assert_eq!(request.metadata["api_key"], "**********", "{}", case.name);
        }
    }
}

/// Fails for a `ProviderError` of the fixture that lacks the field `name`.
#[expect(
    clippy::panic,
    reason = "a test's helper: a case that does not read fails the test, naming it"
)]
fn missing<T>(label: &str, name: &str) -> T {
    panic!("{label}: a ProviderError without {name}")
}

/// The error a `ProviderError` of the contract reads as, with the fields
/// Python's carried.
fn provider_error(failure: &Failure, label: &str) -> ProviderError {
    let provider = failure.provider.clone();
    let provider = provider.unwrap_or_else(|| missing(label, "provider"));
    let before_request = failure.before_request;
    let before_request = before_request.unwrap_or_else(|| missing(label, "before_request"));
    let retryable = failure.retryable;
    let options = ProviderErrorOptions {
        retryable: retryable.unwrap_or_else(|| missing(label, "retryable")),
        provider: provider.clone(),
        status_code: failure.status_code,
        context_overflow: failure.context_overflow,
        before_request,
    };
    let Some(reason) = &failure.reason else {
        assert_eq!(failure.class, "ProviderError", "{label}");
        return ProviderError::new(failure.message.clone(), options);
    };
    assert_eq!(failure.class, "ResponseSchemaError", "{label}");
    let reasons = [
        ResponseSchemaFailure::Unsupported,
        ResponseSchemaFailure::Refusal,
        ResponseSchemaFailure::Truncated,
        ResponseSchemaFailure::InvalidJson,
    ];
    let reason = reasons.into_iter().find(|known| known.as_str() == reason);
    let reason = reason.unwrap_or_else(|| missing(label, "a reason the contract names"));
    let schema = ResponseSchemaErrorOptions { provider };
    let error: ProviderError =
        ResponseSchemaError::new(failure.message.clone(), reason, schema).into();
    // What the contract derives from a schema's failure is what Python's
    // error carried.
    assert_eq!(
        (
            error.retryable,
            error.before_request,
            error.status_code,
            error.context_overflow
        ),
        (options.retryable, before_request, None, None),
        "{label}"
    );
    error
}

#[test]
fn each_result_reads_as_the_contract_s_types_and_writes_back_unchanged() {
    for case in fixture().cases {
        let (name, expected) = (&case.name, &case.expected);
        let given = [
            expected.response.is_some(),
            expected.text.is_some(),
            expected.events.is_some(),
        ];
        let failed = expected.error.is_some();
        match case.call.as_str() {
            "generate" => assert_eq!(given, [!failed, false, false], "{name}"),
            "generate_stream" => assert!(given == [false, true, false] || failed, "{name}"),
            "generate_structured_stream" => {
                assert!(given == [false, false, true] || failed, "{name}");
            }
            other => panic!("{name}: an unknown call {other}"),
        }
        if let Some(response) = &expected.response {
            read_back::<AiResponse>(response, name);
        }
        for event in expected.events.iter().flatten() {
            read_back::<StreamEvent>(event, name);
        }
        let Some(failure) = &expected.error else {
            continue;
        };
        if failure.provider.is_none() {
            // The client could not be built: no error of the contract.
            assert_eq!(failure.class, "OpenAIError", "{name}");
            assert!(case.exchanges.is_empty(), "{name}");
            continue;
        }
        let error = provider_error(failure, name);
        assert_eq!(error.to_string(), failure.text, "{name}");
        assert_eq!(error.provider, "openai", "{name}");
        if error.before_request {
            assert!(case.exchanges.is_empty(), "{name}");
        }
        assert!(
            failure.raised_from.as_deref() != Some(""),
            "{name}: an empty class"
        );
    }
}

#[test]
fn each_exchange_holds_a_request_python_sent_and_one_answer() {
    for case in fixture().cases {
        let name = &case.name;
        for exchange in &case.exchanges {
            let sent = &exchange.request;
            assert_eq!(sent.method, "POST", "{name}");
            assert!(sent.path.ends_with("chat/completions"), "{name}");
            assert!(sent.body.is_object(), "{name}");
            for value in sent.headers.values().flatten() {
                assert!(!value.contains(KEY), "{name}: a key in a header");
            }
            let answer = &exchange.response;
            let bodies = [
                answer.json.is_some(),
                answer.text.is_some(),
                answer.chunks.is_some(),
                answer.failure.is_some(),
            ];
            assert_eq!(bodies.iter().filter(|held| **held).count(), 1, "{name}");
            assert_eq!(answer.status.is_some(), answer.failure.is_none(), "{name}");
            assert_eq!(answer.headers.is_some(), answer.failure.is_none(), "{name}");
            assert_eq!(answer.then.is_some(), answer.chunks.is_some(), "{name}");
            if let Some(wait) = &exchange.wait {
                assert!(wait.seconds > 0.0 && wait.seconds <= 120.0, "{name}");
                assert!(wait.from.starts_with("the "), "{name}");
            }
        }
        // The SDK waits before each next try: after every exchange but the
        // last, and but a redirect, which it follows at once.
        let last = case.exchanges.len().saturating_sub(1);
        for (at, exchange) in case.exchanges.iter().enumerate() {
            let redirected = matches!(exchange.response.status, Some(300..=399));
            let waited = exchange.wait.is_some();
            assert_eq!(waited, at != last && !redirected, "{name}");
        }
    }
}

/// An id Python minted is random: a replay compares its shape.
#[test]
fn each_minted_call_id_is_in_the_result_and_has_python_s_shape() {
    for case in fixture().cases {
        let expected = serde_json::to_string(&case.expected.response).unwrap()
            + &serde_json::to_string(&case.expected.events).unwrap();
        for id in &case.minted_call_ids {
            assert!(expected.contains(&format!("\"{id}\"")), "{}", case.name);
            let drawn = id.rsplit('_').next().unwrap_or_default();
            assert!(id.starts_with("call_"), "{}", case.name);
            assert!(
                drawn.len() == 12 && drawn.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "{}",
                case.name
            );
        }
        let minted = case.kinds.iter().any(|kind| kind == "minted_call_id");
        assert_eq!(minted, !case.minted_call_ids.is_empty(), "{}", case.name);
    }
}

#[test]
fn the_fixture_covers_every_kind_of_case() {
    let fixture = fixture();
    let known: BTreeSet<&str> = KINDS.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    for case in &fixture.cases {
        assert!(names.insert(case.name.as_str()), "{} twice", case.name);
        for kind in &case.kinds {
            assert!(known.contains(kind.as_str()), "{}: {kind}", case.name);
            seen.insert(kind.as_str());
        }
    }
    assert_eq!(seen, known);
    let calls = |call: &str| {
        fixture
            .cases
            .iter()
            .filter(|case| case.call == call)
            .count()
    };
    assert!(calls("generate") >= 100, "{}", calls("generate"));
    assert!(
        calls("generate_stream") >= 4,
        "{}",
        calls("generate_stream")
    );
    assert!(
        calls("generate_structured_stream") >= 35,
        "{}",
        calls("generate_structured_stream")
    );
    assert!(fixture.cases.len() >= 150, "the fixture lost cases");
}
