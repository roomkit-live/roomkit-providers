//! The wire cases captured on `RoomKit`'s Python `OpenAI` provider, read
//! with the contract's types and this crate's configuration:
//! `tests/fixtures/python_chat_cases.json`, written by
//! `scripts/generate_openai_chat_cases.py` (`make python-openai-chat-cases`).
//!
//! Each case holds a configuration, a request, the HTTP exchanges Python had
//! with a fake of the endpoint, and what Python returned. The provider that
//! replays them is not written yet: this test holds the table to its shape,
//! reads every part of it, and requires every kind of case.
//!
//! A body read in several reads is a list of chunks, each the bytes of one
//! read: a JSON text where the bytes are UTF-8, else an object whose `hex`
//! member holds them in hexadecimal, as a read that ends inside a character
//! is. A case that fails says under `compare` what a replay reproduces of
//! the failure and whose words its message holds, and a case that pins a
//! behaviour of Python that is not settled says so under `questioned`: a
//! possible defect reported upstream, or a difference the maintainer
//! decides.

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

/// `OpenAI`'s own endpoint, which a configuration with no base URL calls.
const OPENAI: &str = "https://api.openai.com/v1";

#[derive(Deserialize)]
struct Fixture {
    roomkit_version: String,
    roomkit_commit: Option<String>,
    roomkit_dirty: Option<bool>,
    openai_version: String,
    pydantic_version: String,
    httpx2_version: String,
    python_version: String,
    clock: String,
    notes: BTreeMap<String, String>,
    questioned: Vec<String>,
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
    sdk: Option<String>,
    questioned: Option<String>,
    config: Value,
    catalogue: Option<Catalogue>,
    request: Value,
    request_api_key: Option<String>,
    exchanges: Vec<Exchange>,
    expected: Expected,
    compare: Option<Compare>,
    #[serde(default)]
    minted_call_ids: Vec<String>,
}

/// What the provider read of its model's catalogue entry.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    model: String,
    known: bool,
    tags: Vec<String>,
    reasoning_floor: Option<String>,
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
    url: String,
    headers: BTreeMap<String, Option<String>>,
    timeouts: BTreeMap<String, f64>,
    body: Option<Value>,
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
    chunks: Option<Vec<Chunk>>,
    then: Option<String>,
    failure: Option<String>,
}

/// The bytes of one read: their text where they are UTF-8, else in
/// hexadecimal.
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum Chunk {
    Text(String),
    Bytes { hex: String },
}

impl Chunk {
    /// The bytes read, `None` for a `hex` member that is no hexadecimal.
    fn bytes(&self) -> Option<Vec<u8>> {
        let hex = match self {
            Self::Text(text) => return Some(text.clone().into_bytes()),
            Self::Bytes { hex } => hex.as_bytes(),
        };
        let digits = hex.chunks(2).map(|pair| std::str::from_utf8(pair).ok());
        digits
            .map(|pair| u8::from_str_radix(pair.filter(|pair| pair.len() == 2)?, 16).ok())
            .collect()
    }
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

/// How a replay compares a failure: what it reproduces as written, and
/// whose words the message holds.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compare {
    exactly: Vec<String>,
    message: String,
}

/// The attributes of a `ProviderError` a replay reproduces as written; a
/// response schema's failure adds its `reason`.
const EXACT: [&str; 6] = [
    "class",
    "status_code",
    "retryable",
    "provider",
    "context_overflow",
    "before_request",
];

/// Whose words a failure's message may hold, and whether a replay compares
/// it: `RoomKit`'s own and the server's are, the others are not.
const WORDS: [(&str, bool); 7] = [
    ("roomkit", true),
    ("server", true),
    ("sdk", false),
    ("python_json", false),
    ("python_repr", false),
    ("transport", false),
    ("python", false),
];

/// What the openai SDK says of a failure that has no answer, or of an error
/// event that holds no message.
const SDK_WORDS: [&str; 3] = [
    "Connection error.",
    "Request timed out.",
    "An error occurred during streaming",
];

/// The classes the HTTP client raises for a body it could not read to its
/// end.
const TRANSPORT_ERRORS: [&str; 4] = [
    "RemoteProtocolError",
    "ReadTimeout",
    "ReadError",
    "ConnectError",
];

/// The two kinds of case that pin a behaviour of Python that is not
/// settled, as their sentence starts.
const POSSIBLE_DEFECT: &str = "A possible defect: ";
const TO_DECIDE: &str = "A difference to decide: ";

/// The cases that pin a behaviour of Python that is not settled: reported
/// upstream as a possible defect, or a point where the Rust provider is
/// planned otherwise and the maintainer decides. A replay may set them
/// aside, or expect otherwise once each is settled: a case joins or leaves
/// this list with that decision.
const QUESTIONED: [&str; 38] = [
    "answer_200_that_is_no_completion",
    "authorization_among_the_default_headers",
    "base_url_with_a_trailing_slash_and_a_query",
    "choices_that_are_no_list",
    "extra_body_replacing_the_request_s_own",
    "key_of_the_request",
    "redirect",
    "redirect_301",
    "redirect_302_to_another_host",
    "redirect_303",
    "redirect_308",
    "redirects_past_the_hop_limit",
    "status_400_whose_body_is_lost",
    "status_400_whose_body_is_lost_on_every_try",
    "stream_answered_with_a_json_error_object",
    "stream_call_id_that_is_no_text",
    "stream_last_event_without_its_blank_line",
    "stream_last_event_without_its_line_end",
    "stream_status_400_whose_body_is_lost",
    "stream_that_ends_without_an_ending",
    "stream_that_starts_with_a_byte_order_mark",
    "stream_tool_call_that_ends_without_an_ending",
    "stream_usage_cached_tokens_as_text",
    "stream_usage_completion_tokens_as_text",
    "stream_usage_counts_that_are_true",
    "stream_usage_reasoning_tokens_as_text",
    "token_bound_of_zero_in_the_request",
    "usage_cache_write_written_as_text_in_the_details",
    "usage_cached_tokens_as_a_word",
    "usage_cached_tokens_as_text",
    "usage_completion_tokens_as_a_word",
    "usage_completion_tokens_as_text",
    "usage_counts_below_zero",
    "usage_counts_that_are_true",
    "usage_counts_with_a_fraction",
    "usage_prompt_tokens_as_text",
    "usage_reasoning_tokens_as_a_word",
    "usage_reasoning_tokens_as_text",
];

/// The cases of [`QUESTIONED`] that claim no defect of Python's.
const QUESTIONED_TO_DECIDE: [&str; 3] = [
    "authorization_among_the_default_headers",
    "redirect",
    "redirect_308",
];

/// The one case whose URL is not its base URL followed by the path of Chat
/// Completions: the SDK writes the path after the base URL's query.
const MANGLED: (&str, &str) = (
    "base_url_with_a_trailing_slash_and_a_query",
    "https://llm.example/gateway/v1/?route=bluechat/completions",
);

/// What a fixture must show of the provider: each kind, in at least one
/// case. A regenerated fixture that loses one fails here.
const KINDS: [&str; 78] = [
    // A request and its answer.
    "plain",
    "system_prompt",
    "turns",
    "image_data_uri",
    "image_url",
    "odd_shapes",
    // Tools.
    "tools",
    "tool_call",
    "tool_call_streamed",
    "tool_call_folding",
    "tool_calls_several",
    "tool_call_unreadable",
    "call_arguments",
    "tool_round_replayed",
    "tool_result_parts",
    "minted_call_id",
    // Reasoning, the temperature and the token bound.
    "reasoning_effort",
    "reasoning_switch",
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
    // Streams and usage.
    "stream",
    "stream_usage_on",
    "stream_usage_off",
    "stream_cut_anywhere",
    "stream_cut_inside_a_character",
    "stream_shapes",
    "usage_cached",
    "usage_reasoning",
    "usage_shapes",
    // Endings.
    "finish_stop",
    "finish_length",
    "finish_content_filter",
    "finish_tool_calls",
    "empty_answer",
    // Failures.
    "before_request",
    "status_204",
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
    "status_504",
    "status_529",
    "retry_after_seconds",
    "retry_after_date",
    "context_overflow",
    "not_json",
    "not_a_completion",
    "error_object",
    "transient_words",
    "body_lost",
    "stream_cut",
    "stream_error_event",
    "connection_refused",
    "timeout",
    // Retries of the SDK.
    "retry_succeeds",
    "retries_exhausted",
    "retry_wait",
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
fn the_fixture_says_where_it_comes_from_and_how_it_is_written() {
    let fixture = fixture();
    for version in [
        &fixture.roomkit_version,
        &fixture.openai_version,
        &fixture.pydantic_version,
        &fixture.httpx2_version,
        &fixture.python_version,
    ] {
        assert!(
            version.starts_with(|c: char| c.is_ascii_digit()),
            "{version}"
        );
    }
    if let Some(commit) = &fixture.roomkit_commit {
        let hexadecimal = commit.bytes().all(|byte| byte.is_ascii_hexdigit());
        assert!(commit.len() == 40 && hexadecimal, "{commit}");
    }
    // Whether the files were clean is said with the commit, and only with it.
    assert_eq!(
        fixture.roomkit_dirty.is_some(),
        fixture.roomkit_commit.is_some()
    );
    assert_eq!(fixture.clock, "2026-01-01T00:00:00Z");
    for note in [
        "environment",
        "provenance",
        "exchanges.request",
        "exchanges.response",
        "compare",
        "questioned",
        "sdk",
    ] {
        assert!(fixture.notes.contains_key(note), "no note on {note}");
    }
    assert!(fixture.notes["environment"].contains("OPENAI_*"));
    assert!(fixture.notes["exchanges.response"].contains("hex"));
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

/// A case that calls `OpenAI`'s own endpoint says what the provider read of
/// its model's catalogue entry; behind a base URL it reads none.
#[test]
fn a_case_on_openai_s_endpoint_holds_the_catalogue_facts_it_rests_on() {
    let read = [
        "responses_only",
        "chat_tools_refused",
        "tools_reasoning_none",
        "tools_reasoning_effort",
    ];
    let mut models = BTreeSet::new();
    for case in fixture().cases {
        let own = case.config.get("base_url").is_none();
        assert_eq!(case.catalogue.is_some(), own, "{}", case.name);
        let Some(catalogue) = case.catalogue else {
            continue;
        };
        assert_eq!(catalogue.model, case.config["model"], "{}", case.name);
        assert!(
            catalogue
                .tags
                .iter()
                .all(|tag| read.contains(&tag.as_str())),
            "{}",
            case.name
        );
        assert!(
            catalogue.known || (catalogue.tags.is_empty() && catalogue.reasoning_floor.is_none()),
            "{}",
            case.name
        );
        models.insert(catalogue.model);
    }
    for model in [
        "gpt-4o",
        "gpt-5",
        "gpt-5.1",
        "gpt-5.4",
        "gpt-5.5-pro",
        "gpt-6-astra",
        "o3",
    ] {
        assert!(models.contains(model), "no case on {model}");
    }
}

/// What the configuration says of the token bound and of the temperature is
/// what Python sent, unless the configuration's extra body says otherwise.
#[test]
fn the_switches_of_a_configuration_are_the_ones_python_sent() {
    for case in fixture().cases {
        let config: OpenAiConfig = serde_json::from_value(case.config.clone()).unwrap();
        let written = serde_json::to_value(&config).unwrap();
        let replaced = config
            .extra_body
            .is_some_and(|body| body.contains_key("temperature"));
        for exchange in &case.exchanges {
            let Some(body) = exchange.request.body.as_ref().and_then(Value::as_object) else {
                continue;
            };
            let bound = ["max_tokens", "max_completion_tokens"].map(|name| body.contains_key(name));
            if bound != [false, false] {
                let modern = written["use_max_completion_tokens"] == true;
                assert_eq!(bound, [!modern, modern], "{}", case.name);
            }
            if !replaced {
                assert_eq!(
                    written["supports_custom_temperature"],
                    body.contains_key("temperature"),
                    "{}",
                    case.name
                );
            }
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
            // No error of the contract: the SDK's own, or a raw Python
            // exception.
            assert_ne!(failure.class, "ProviderError", "{name}");
            assert_ne!(failure.class, "ResponseSchemaError", "{name}");
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

/// Every text of `value`, itself included.
fn texts_in(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::String(text) => found.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| texts_in(item, found)),
        Value::Object(members) => members.values().for_each(|item| texts_in(item, found)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Every text the fake sent in the exchanges of a case: a text body, each
/// text of a JSON body, and each text of the JSON a stream's `data` lines
/// hold.
fn answered_texts(exchanges: &[Exchange]) -> Vec<String> {
    let mut found = Vec::new();
    for answer in exchanges.iter().map(|exchange| &exchange.response) {
        found.extend(answer.text.iter().map(|text| text.trim().to_owned()));
        answer
            .json
            .iter()
            .for_each(|body| texts_in(body, &mut found));
        let reads = answer.chunks.iter().flatten().filter_map(Chunk::bytes);
        let sent: Vec<u8> = reads.flatten().collect();
        for line in String::from_utf8_lossy(&sent).lines() {
            let data = line.strip_prefix("data:").unwrap_or(line);
            if let Ok(event) = serde_json::from_str::<Value>(data) {
                texts_in(&event, &mut found);
            }
        }
    }
    found
}

/// Whose words the message of `failure` holds, read from the failure itself
/// and from what the fake sent: the label the fixture must give it.
fn words_of(failure: &Failure, sent: &[String]) -> &'static str {
    let message = failure.message.as_str();
    let cause = failure.raised_from.as_deref();
    if failure.class == "OpenAIError" || SDK_WORDS.contains(&message) {
        return "sdk";
    }
    if failure.provider.is_none() {
        return "python";
    }
    if cause == Some("JSONDecodeError") {
        return "python_json";
    }
    if cause.is_some_and(|class| TRANSPORT_ERRORS.contains(&class)) {
        return "transport";
    }
    if let Some(body) = message.strip_prefix("Error code: ") {
        // Python's repr of the body follows the status, when a body came.
        return if body.contains(" - ") {
            "python_repr"
        } else {
            "sdk"
        };
    }
    if sent.iter().any(|text| text == message) {
        return "server";
    }
    if matches!(cause, None | Some("ValueError")) {
        "roomkit"
    } else {
        "python"
    }
}

/// Every failure says how a replay compares it, and no other case does:
/// what it reproduces as written, the reason of a response schema's failure
/// included, and whose words its message holds.
#[test]
fn each_failure_says_what_a_replay_reproduces_of_it() {
    let (mut compared, mut failures, mut reasons) = (0, 0, 0);
    for case in fixture().cases {
        let (name, expected) = (&case.name, &case.expected);
        assert_eq!(case.compare.is_some(), expected.error.is_some(), "{name}");
        let (Some(compare), Some(failure)) = (&case.compare, &expected.error) else {
            continue;
        };
        failures += 1;
        // The label is the one the failure and the answers stand for.
        let words = words_of(failure, &answered_texts(&case.exchanges));
        assert_eq!(compare.message, words, "{name}");
        let is_compared = WORDS.iter().find(|(known, _)| *known == words);
        compared += usize::from(is_compared.is_some_and(|(_, compared)| *compared));
        if failure.provider.is_none() {
            // A failure no provider reproduces.
            assert!(compare.exactly.is_empty(), "{name}");
            continue;
        }
        let reason = failure.reason.as_ref().map(|_| "reason");
        reasons += usize::from(reason.is_some());
        let given = [
            ("events", expected.events.is_some()),
            ("text", expected.text.is_some()),
        ];
        let given = given.iter().filter(|(_, held)| *held).map(|(key, _)| *key);
        let exactly: Vec<&str> = EXACT.into_iter().chain(reason).chain(given).collect();
        assert_eq!(compare.exactly, exactly, "{name}");
        // A failure before any request holds RoomKit's words alone.
        if failure.before_request == Some(true) {
            assert_eq!(words, "roomkit", "{name}");
        }
    }
    assert!(failures >= 112, "{failures} failures");
    assert!(compared >= 45, "{compared} messages compared");
    assert!(reasons >= 15, "{reasons} failures of a response schema");
}

/// The cases that pin a behaviour of Python that is not settled are the
/// ones this test lists, each with a sentence that starts with its kind.
#[test]
fn the_questioned_cases_are_the_ones_listed() {
    let fixture = fixture();
    let questioned = |case: &&Case| case.questioned.is_some();
    let marked = fixture.cases.iter().filter(questioned);
    let mut marked: Vec<&str> = marked.map(|case| case.name.as_str()).collect();
    marked.sort_unstable();
    assert_eq!(marked, QUESTIONED);
    let mut listed: Vec<&str> = fixture.questioned.iter().map(String::as_str).collect();
    listed.sort_unstable();
    assert_eq!(listed, QUESTIONED);
    let mut to_decide = Vec::new();
    for case in fixture.cases.iter().filter(questioned) {
        let sentence = case.questioned.as_deref().unwrap_or_default();
        assert!(sentence.ends_with('.'), "{}", case.name);
        if sentence.starts_with(TO_DECIDE) {
            to_decide.push(case.name.as_str());
        } else {
            assert!(sentence.starts_with(POSSIBLE_DEFECT), "{}", case.name);
        }
    }
    to_decide.sort_unstable();
    assert_eq!(to_decide, QUESTIONED_TO_DECIDE);
    assert!(fixture.notes["questioned"].contains(POSSIBLE_DEFECT.trim_end_matches(": ")));
    assert!(fixture.notes["questioned"].contains(TO_DECIDE.trim_end_matches(": ")));
}

/// Each request went where its configuration says: the base URL, `OpenAI`'s
/// own when none is given, then the path of Chat Completions; or where the
/// answer before it redirected it.
#[test]
fn each_request_was_sent_to_the_url_its_case_expects() {
    for case in fixture().cases {
        let name = &case.name;
        let base = case.config["base_url"].as_str().unwrap_or(OPENAI);
        let mut expected = format!("{}/chat/completions", base.trim_end_matches('/'));
        if *name == MANGLED.0 {
            assert!(case.questioned.is_some(), "{name}");
            MANGLED.1.clone_into(&mut expected);
        }
        for exchange in &case.exchanges {
            assert_eq!(exchange.request.url, expected, "{name}");
            let answer = &exchange.response;
            if matches!(answer.status, Some(300..=399)) {
                let headers = answer.headers.as_ref().unwrap();
                expected.clone_from(&headers["location"]);
                assert!(case.questioned.is_some(), "{name}");
            }
        }
        // A case with no base URL shows OpenAI's own endpoint.
        if case.config.get("base_url").is_none() {
            for exchange in &case.exchanges {
                assert!(exchange.request.url.starts_with(OPENAI), "{name}");
            }
        }
    }
}

#[test]
fn each_exchange_holds_a_request_python_sent_and_one_answer() {
    let endings = [
        "the stream ends",
        "the connection is lost",
        "the read times out",
    ];
    for case in fixture().cases {
        let name = &case.name;
        for exchange in &case.exchanges {
            let sent = &exchange.request;
            // A redirect the SDK follows as a GET carries no body.
            assert_eq!(sent.method == "POST", sent.body.is_some(), "{name}");
            assert!(["POST", "GET"].contains(&sent.method.as_str()), "{name}");
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
            if let Some(then) = &answer.then {
                assert!(endings.contains(&then.as_str()), "{name}: {then}");
            }
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

/// A chunk holds any bytes: a text is the UTF-8 it stands for, and bytes
/// that are no UTF-8 are written in hexadecimal. Where reads cut inside
/// characters, the reads together are the whole text.
#[test]
fn each_chunk_reads_as_the_bytes_of_one_read() {
    let mut cut_inside = 0;
    for case in fixture().cases {
        let name = &case.name;
        for exchange in &case.exchanges {
            let Some(chunks) = &exchange.response.chunks else {
                continue;
            };
            let mut whole = Vec::new();
            let mut partial = 0;
            for chunk in chunks {
                let bytes = chunk.bytes().unwrap_or_else(|| panic!("{name}: a chunk"));
                if matches!(chunk, Chunk::Bytes { .. }) {
                    // Bytes that read as a text are written as that text.
                    assert!(std::str::from_utf8(&bytes).is_err(), "{name}");
                    partial += 1;
                }
                whole.extend(bytes);
            }
            let inside = case
                .kinds
                .iter()
                .any(|kind| kind == "stream_cut_inside_a_character");
            assert_eq!(partial > 0, inside, "{name}");
            if inside {
                assert!(std::str::from_utf8(&whole).is_ok(), "{name}");
                cut_inside += partial;
            }
        }
    }
    // Reads that end inside characters of two, three and four bytes, in a
    // text, in a call's arguments and in reasoning, and one byte at a time.
    assert!(
        cut_inside >= 4 * 9,
        "{cut_inside} reads cut inside a character"
    );
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
        if case.kinds.iter().any(|kind| kind == "minted_call_id") {
            assert!(!case.minted_call_ids.is_empty(), "{}", case.name);
        }
    }
}

/// Where the SDK decides, the case says so: its retries, a redirect it
/// follows, and a failure whose words are its own.
#[test]
fn a_case_the_sdk_decides_says_so() {
    for case in fixture().cases {
        let tried_again = case.exchanges.len() > 1;
        let its_words = case
            .compare
            .as_ref()
            .is_some_and(|compare| compare.message == "sdk");
        if tried_again || its_words {
            assert!(case.sdk.is_some(), "{}", case.name);
        }
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
    assert!(calls("generate") >= 203, "{}", calls("generate"));
    assert!(
        calls("generate_stream") >= 4,
        "{}",
        calls("generate_stream")
    );
    assert!(
        calls("generate_structured_stream") >= 101,
        "{}",
        calls("generate_structured_stream")
    );
    assert!(fixture.cases.len() >= 308, "the fixture lost cases");
}
