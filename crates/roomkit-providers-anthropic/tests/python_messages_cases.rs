//! The wire cases captured on `RoomKit`'s Python Anthropic provider, read
//! with the contract's types and this crate's configuration:
//! `tests/fixtures/python_messages_cases.json`, written by
//! `scripts/generate_anthropic_messages_cases.py`.
//!
//! Each case holds a configuration, a request, the HTTP exchanges Python had
//! with a fake of the endpoint, and what Python returned. The provider
//! replays them in its own tests; this test holds the table to its shape,
//! reads every part of it, and requires every kind of case.
//!
//! What every fixture of recorded cases holds is checked by the harness of
//! `roomkit-providers-client` (`harness::shape`), which says how a body, a
//! failure and a questioned case are written. What is Anthropic's own is
//! checked here: its configuration, its catalogue, what a provider answers
//! of itself, the URLs it asks, and the cases the fixture must hold.

use std::collections::BTreeSet;

use roomkit_providers_anthropic::AnthropicConfig;
use roomkit_providers_client::harness::shape::{Case, Fixture, Shape};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_messages_cases.json");

/// The key of every configuration, and the keys a request carries.
const KEY: &str = "placeholder-not-a-key";
const REQUEST_KEYS: [&str; 4] = [
    "placeholder-not-a-key-either",
    "placeholder-not-a-key-of-another",
    // A key written in white space, and one that is white space alone.
    " placeholder-not-a-key-in-spaces ",
    "   ",
];

/// Anthropic's own endpoint, which a configuration with no base URL calls.
const ANTHROPIC: &str = "https://api.anthropic.com";

/// The classes of the anthropic SDK a call may raise as they are.
const SDK_ERRORS: [&str; 17] = [
    "AnthropicError",
    "APIConnectionError",
    "APITimeoutError",
    "APIResponseValidationError",
    "APIStatusError",
    "BadRequestError",
    "AuthenticationError",
    "PermissionDeniedError",
    "NotFoundError",
    "ConflictError",
    "RequestTooLargeError",
    "UnprocessableEntityError",
    "RateLimitError",
    "InternalServerError",
    "ServiceUnavailableError",
    "DeadlineExceededError",
    "OverloadedError",
];

/// The classes of the SDK a failure with no answer is raised from, in the
/// SDK's own words.
const SDK_CAUSES: [&str; 2] = ["APIConnectionError", "APITimeoutError"];

/// How the anthropic SDK's own sentences start, where it raises a Python
/// exception of no class of its own.
const SDK_SENTENCES: [&str; 4] = [
    "Unable to parse tool parameter JSON from model.",
    "Unexpected event order, got ",
    "Unexpected event runtime type, ",
    "\"Could not resolve authentication method.",
];

/// The tags of a model's catalogue entry the provider reads.
const READ_TAGS: [&str; 7] = [
    "deferred_tools",
    "effort_refused",
    "effort_no_xhigh",
    "effort_up_to_high",
    "thinking_off_disabled",
    "thinking_off_between_tools",
    "thinking_off_effort_high",
];

/// The cases that pin a behaviour of Python that is not settled: a possible
/// defect, or a point where the Rust provider is planned otherwise and the
/// maintainer decides. A replay may set them aside, or expect otherwise
/// once each is settled: a case joins or leaves this list with that
/// decision.
const QUESTIONED: [&str; 48] = [
    "answer_200_that_is_an_error_object",
    "answer_that_ends_inside_a_text",
    "answer_that_starts_with_a_byte_order_mark",
    "base_url_that_is_empty",
    "base_url_with_a_query",
    "extra_header_in_place_of_the_key",
    "extra_header_in_place_of_the_key_of_the_request",
    "key_of_the_request_in_white_space",
    "key_of_the_request_that_is_blank",
    "list_models_page_that_says_more_follow",
    "list_models_redirect_to_another_host",
    "redirect_301",
    "redirect_302",
    "redirect_302_to_another_host",
    "redirect_303",
    "redirect_307",
    "redirect_307_to_another_host",
    "redirect_308",
    "redirect_from_anthropic_s_endpoint_to_another_host",
    "redirect_to_a_relative_location",
    "redirects_past_the_hop_limit",
    "status_400_whose_body_is_lost",
    "status_400_whose_body_is_lost_with_retries",
    "status_400_whose_body_starts_with_a_byte_order_mark",
    "stream_answer_200_that_is_an_error_object",
    "stream_last_event_without_its_blank_line",
    "stream_of_a_message_start_alone",
    "stream_that_ends_before_its_stop_reason",
    "stream_that_ends_inside_a_text",
    "stream_that_ends_inside_an_event",
    "stream_that_ends_inside_its_end_event",
    "stream_that_ends_without_its_end_event",
    "stream_that_starts_with_a_byte_order_mark",
    "stream_tool_call_closed_before_the_stream_ends",
    "stream_tool_call_that_ends_without_an_ending",
    "stream_tool_call_whose_arguments_are_blank",
    "stream_tool_call_whose_arguments_are_no_json",
    "stream_tool_call_whose_input_comes_whole_at_its_start",
    "stream_tool_call_without_an_id",
    "stream_tool_calls_that_share_an_id_and_are_left_open",
    "stream_usage_counts_written_as_text",
    "token_bound_of_zero_in_the_request",
    "tool_call_whose_arguments_are_no_json",
    "usage_a_cache_counter_written_as_text",
    "usage_counts_below_zero",
    "usage_counts_that_are_true",
    "usage_counts_written_as_text",
    "usage_output_written_as_text",
];

/// The cases of [`QUESTIONED`] that claim no defect of Python's.
const QUESTIONED_TO_DECIDE: [&str; 5] = [
    "extra_header_in_place_of_the_key",
    "extra_header_in_place_of_the_key_of_the_request",
    "redirect_307",
    "redirect_308",
    "redirect_to_a_relative_location",
];

/// The one case whose request the contract does not read: it holds a role
/// the contract does not name.
const UNREAD: [&str; 1] = ["turn_whose_role_the_provider_does_not_know"];

/// What a fixture must show of the provider: each kind, in at least one
/// case. A regenerated fixture that loses one fails here.
const KINDS: [&str; 106] = [
    "base_url",
    "before_request",
    "body_lost",
    "byte_order_mark",
    "cache_marks",
    "cache_off",
    "call_arguments",
    "call_ids",
    "connection_refused",
    "context_overflow",
    "deferred_tools",
    "effort",
    "empty_answer",
    "error_object",
    "extra_headers",
    "fence",
    "finish_end_turn",
    "finish_max_tokens",
    "finish_model_context_window_exceeded",
    "finish_none",
    "finish_pause_turn",
    "finish_refusal",
    "finish_stop_sequence",
    "finish_tool_use",
    "image_data_uri",
    "image_url",
    "listing",
    "listing_content_type",
    "listing_pages",
    "max_tokens",
    "no_temperature",
    "not_a_message",
    "not_a_stream",
    "not_json",
    "odd_shapes",
    "plain",
    "redirect",
    "redirect_to_another_host",
    "references",
    "refusal",
    "request_key",
    "request_keys_several",
    "response_schema",
    "response_schema_invalid",
    "response_schema_refusal",
    "response_schema_truncated",
    "response_schema_with_tools",
    "retries_exhausted",
    "retry_after_date",
    "retry_after_seconds",
    "retry_succeeds",
    "retry_wait",
    "status_400",
    "status_401",
    "status_402",
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
    "stream",
    "stream_cut",
    "stream_cut_anywhere",
    "stream_cut_inside_a_character",
    "stream_ends_inside_an_event",
    "stream_error_event",
    "stream_events_ignored",
    "stream_events_named",
    "stream_without_its_end",
    "system_prompt",
    "temperature",
    "thinking_adaptive",
    "thinking_budget",
    "thinking_cut",
    "thinking_off",
    "thinking_read",
    "thinking_redacted",
    "thinking_replayed",
    "thinking_sent",
    "thinking_signed",
    "thinking_unsaid",
    "timeout",
    "timeouts",
    "tool_call",
    "tool_call_cut",
    "tool_call_folding",
    "tool_call_streamed",
    "tool_calls_several",
    "tool_name",
    "tool_result",
    "tool_result_error",
    "tool_result_image",
    "tools",
    "turns",
    "usage_cache_read",
    "usage_cache_write",
    "usage_reasoning",
    "usage_shapes",
    "vision",
];

/// What the fixture states of itself, beside what every fixture holds.
const SHAPE: Shape<'static> = Shape {
    provider: "anthropic",
    sdk_version: "anthropic_version",
    environment: "ANTHROPIC_*",
    notes: &[
        "environment",
        "provenance",
        "supports",
        "before",
        "exchanges.request",
        "exchanges.response",
        "compare",
        "questioned",
        "sdk",
    ],
    own_members: &["supports_deferred_tools", "before"],
    key: KEY,
    request_keys: &REQUEST_KEYS,
    unread: &UNREAD,
    sdk_errors: &SDK_ERRORS,
    sdk_words: &[],
    sdk_causes: &SDK_CAUSES,
    sdk_sentences: &SDK_SENTENCES,
    raises_an_event_s_data: true,
    longest_wait: 4_294_967.0,
};

fn fixture() -> Fixture {
    Fixture::read(FIXTURE, &SHAPE)
}

/// The configuration of `case`.
#[expect(
    clippy::panic,
    reason = "a test's helper: a case that does not read fails the test, naming it"
)]
fn config_of(case: &Case) -> AnthropicConfig {
    serde_json::from_value(case.config.clone())
        .unwrap_or_else(|error| panic!("{}: {error}", case.name))
}

/// The base URL of `case` as its configuration keeps it, `None` for a
/// configuration that names none.
fn kept_base_url(case: &Case) -> Option<String> {
    let written = serde_json::to_value(config_of(case)).ok()?;
    let kept = written.get("base_url").and_then(Value::as_str);
    kept.map(str::to_owned)
}

#[test]
fn the_fixture_says_where_it_comes_from_and_how_it_is_written() {
    fixture().says_where_it_comes_from(&SHAPE);
}

#[test]
fn each_configuration_reads_as_an_anthropic_configuration() {
    for case in fixture().cases {
        let config = config_of(&case);
        assert_eq!(config.model, case.config["model"], "{}", case.name);
        let key = case.config["api_key"].as_str().unwrap();
        assert!(key.is_empty() || key == KEY, "{}", case.name);
        assert_eq!(config.api_key, key, "{}", case.name);
    }
}

/// Every case says what the provider read of its model's catalogue entry,
/// which it reads whatever the endpoint.
#[test]
fn a_case_holds_the_catalogue_facts_it_rests_on() {
    let mut models = BTreeSet::new();
    for case in fixture().cases {
        let name = &case.name;
        let catalogue = case
            .catalogue
            .unwrap_or_else(|| panic!("{name}: no catalogue"));
        assert_eq!(catalogue.model, case.config["model"], "{name}");
        let read = |tag: &String| READ_TAGS.contains(&tag.as_str());
        assert!(catalogue.tags.iter().all(read), "{name}");
        let nothing = catalogue.tags.is_empty() && catalogue.reasoning_floor.is_none();
        assert!(catalogue.known || nothing, "{name}");
        models.insert(catalogue.model);
    }
    for model in [
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-sonnet-4-6",
        "claude-opus-4-5",
        "claude-haiku-4-5",
        "claude-opus-4-1",
        "claude-opus-9",
        "local-model",
    ] {
        assert!(models.contains(model), "no case on {model}");
    }
}

/// Each case says what its provider answers of itself: whether its model
/// takes images, and whether it holds a tool declared and unseen, which it
/// does on Anthropic's own endpoint alone, for a model the catalogue tags.
#[test]
fn a_case_says_what_its_provider_answers_of_itself() {
    let mut said = BTreeSet::new();
    for case in fixture().cases {
        let name = &case.name;
        let sees = case
            .supports_vision
            .unwrap_or_else(|| panic!("{name}: no vision"));
        let holds = case.own["supports_deferred_tools"].as_bool();
        let holds = holds.unwrap_or_else(|| panic!("{name}: no deferred tools"));
        let tagged = case
            .catalogue
            .as_ref()
            .is_some_and(|catalogue| catalogue.tags.iter().any(|tag| tag == "deferred_tools"));
        assert!(tagged || !holds, "{name}");
        said.insert((sees, holds, case.config.get("base_url").is_some()));
    }
    let shown: Vec<(bool, bool, bool)> = said.into_iter().collect();
    assert_eq!(
        shown,
        [
            (false, false, false),
            (false, false, true),
            (true, false, false),
            (true, false, true),
            (true, true, false),
            // Anthropic's own endpoint, written out.
            (true, true, true),
        ]
    );
}

/// What the configuration says of thinking and of the temperature is what
/// Python sent: a turn that thinks sends no temperature, and any other
/// sends one where the configuration says the model takes it.
#[test]
fn the_switches_of_a_configuration_are_the_ones_python_sent() {
    for case in fixture().cases {
        let written = serde_json::to_value(config_of(&case)).unwrap();
        for exchange in &case.exchanges {
            let Some(body) = exchange.request.body.as_ref().and_then(Value::as_object) else {
                continue;
            };
            let thinking = body
                .get("thinking")
                .and_then(|thinking| thinking["type"].as_str());
            let heated = body.contains_key("temperature");
            match thinking {
                Some(kind @ ("adaptive" | "enabled")) => {
                    let adaptive = written["use_adaptive_thinking"] == true;
                    assert_eq!(kind == "adaptive", adaptive, "{}", case.name);
                    assert!(!heated, "{}", case.name);
                }
                Some(_) | None => {
                    assert_eq!(
                        written["supports_custom_temperature"], heated,
                        "{}",
                        case.name
                    );
                }
            }
            assert_eq!(body["stream"], true, "{}", case.name);
        }
    }
}

/// The timeouts of a configuration are the ones each request was sent
/// with: its own for a read, a write and the pool, the other to connect.
#[test]
fn the_timeouts_of_a_configuration_are_the_ones_python_sent() {
    fixture().the_timeouts_are_the_ones_python_sent(|config| {
        let config = serde_json::from_value::<AnthropicConfig>(config.clone()).ok()?;
        Some((config.timeout, config.connect_timeout))
    });
}

#[test]
fn each_request_reads_as_the_contract_s_request_and_writes_back_unchanged() {
    fixture().each_request_reads_as_the_contract_s(&SHAPE);
}

#[test]
fn each_result_reads_as_the_contract_s_types_and_writes_back_unchanged() {
    fixture().each_result_reads_as_the_contract_s(&SHAPE);
}

/// Every failure says how a replay compares it, and no other case does:
/// what it reproduces as written, the reason of a response schema's failure
/// included, and whose words its message holds.
#[test]
fn each_failure_says_what_a_replay_reproduces_of_it() {
    let failures = fixture().each_failure_says_what_a_replay_reproduces(&SHAPE);
    assert!(failures.all >= 122, "{} failures", failures.all);
    assert!(
        failures.compared >= 28,
        "{} messages compared",
        failures.compared
    );
    assert!(
        failures.of_a_schema >= 9,
        "{} failures of a response schema",
        failures.of_a_schema
    );
}

/// The cases that pin a behaviour of Python that is not settled are the
/// ones this test lists, each with a sentence that starts with its kind.
#[test]
fn the_questioned_cases_are_the_ones_listed() {
    fixture().the_questioned_cases_are(&QUESTIONED, &QUESTIONED_TO_DECIDE);
}

/// The URL the first request of `case` goes to: its base URL as the
/// configuration keeps it, Anthropic's own when it names none, then the
/// path of the Messages API, or of the model listing, which asks a
/// thousand models at once. The SDK reads an empty base URL as none, and
/// writes the path after a query the base URL holds: both are questioned.
#[expect(
    clippy::panic,
    reason = "a test's helper: a case that does not read fails the test, naming it"
)]
fn asked_at(case: &Case) -> String {
    let kept = kept_base_url(case);
    let odd = kept
        .as_deref()
        .is_some_and(|kept| kept.is_empty() || kept.contains('?'));
    assert!(!odd || case.questioned.is_some(), "{}", case.name);
    let base = kept.filter(|kept| !kept.is_empty());
    let base = base.as_deref().unwrap_or(ANTHROPIC);
    let path = if case.call == "list_models" {
        "/v1/models?limit=1000"
    } else {
        "/v1/messages"
    };
    // As the client writes it: its scheme and its host in lower case,
    // without the port its scheme implies.
    let asked = reqwest::Url::parse(&format!("{base}{path}"));
    asked
        .unwrap_or_else(|error| panic!("{}: {error}", case.name))
        .into()
}

/// Each request went where its configuration says: the base URL, without
/// the path of the Messages API it may end with, Anthropic's own when none
/// is given, then that path, or the model listing's; or where the answer
/// before it redirected it.
#[test]
fn each_request_was_sent_to_the_url_its_case_expects() {
    let fixture = fixture();
    fixture.each_request_was_sent_where_its_case_expects(asked_at);
    // A case with no base URL shows Anthropic's own endpoint.
    for case in &fixture.cases {
        if case.config.get("base_url").is_none() {
            let first = case.exchanges.first();
            let own = first.is_none_or(|first| first.request.url.starts_with(ANTHROPIC));
            assert!(own, "{}", case.name);
        }
    }
}

/// Each request carries the headers the provider and the SDK decide: the
/// key, named by whose it is, the version of the API, and what it accepts;
/// a request with a body says what its body is.
#[test]
fn each_request_carries_the_headers_python_sent() {
    for case in fixture().cases {
        let name = &case.name;
        let configured = case.config.get("extra_headers").and_then(Value::as_object);
        let configured = configured.map(serde_json::Map::len).unwrap_or_default();
        for exchange in &case.exchanges {
            let sent = &exchange.request.headers;
            for kept in [
                "x-api-key",
                "authorization",
                "anthropic-version",
                "content-type",
                "accept",
            ] {
                assert!(sent.contains_key(kept), "{name}: no {kept}");
            }
            assert!(sent.len() <= 5 + configured, "{name}: {sent:?}");
            // A redirect the SDK follows as a GET keeps what the request it
            // stands for said of its body.
            if exchange.request.method == "POST" {
                let json = Some("application/json".to_owned());
                assert_eq!(sent["content-type"], json, "{name}");
            }
        }
    }
}

/// The calls made on the provider of a case before the case's own are each
/// a request, the key it carried, its exchanges and what it gave.
#[test]
fn the_calls_before_a_case_s_own_are_whole() {
    let mut before = 0;
    for case in fixture().cases {
        let Some(calls) = case.own.get("before") else {
            continue;
        };
        for call in calls.as_array().unwrap() {
            let members: BTreeSet<&str> = call
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let all = BTreeSet::from(["request", "request_api_key", "exchanges", "expected"]);
            assert!(members.is_subset(&all), "{}: {members:?}", case.name);
            assert!(call["exchanges"].is_array() && call["expected"].is_object());
            before += 1;
        }
        assert!(case.kinds.iter().any(|kind| kind == "request_keys_several"));
    }
    assert!(before >= 6, "{before} calls before a case's own");
}

#[test]
fn each_exchange_holds_a_request_python_sent_and_one_answer() {
    fixture().each_exchange_holds_a_request_and_one_answer(&SHAPE);
}

/// A chunk holds any bytes: a text is the UTF-8 it stands for, and bytes
/// that are no UTF-8 are written in hexadecimal. Where reads cut inside
/// characters, the reads together are the whole text.
#[test]
fn each_chunk_reads_as_the_bytes_of_one_read() {
    let cut_inside = fixture().each_chunk_reads_as_the_bytes_of_one_read();
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
    let fixture = fixture();
    fixture.each_minted_call_id_has_python_s_shape(None);
    let minting = |case: &&Case| !case.minted_call_ids.is_empty();
    assert!(fixture.cases.iter().filter(minting).count() >= 4);
}

/// Where the SDK decides, the case says so: its retries, a redirect it
/// follows, and a failure whose words are its own.
#[test]
fn a_case_the_sdk_decides_says_so() {
    fixture().a_case_the_sdk_decides_says_so();
}

#[test]
fn the_fixture_covers_every_kind_of_case() {
    let fixture = fixture();
    fixture.covers_every_kind_of_case(&KINDS);
    let calls = |call: &str| fixture.calls(call);
    assert!(calls("generate") >= 248, "{}", calls("generate"));
    assert!(
        calls("generate_stream") >= 6,
        "{}",
        calls("generate_stream")
    );
    assert!(
        calls("generate_structured_stream") >= 105,
        "{}",
        calls("generate_structured_stream")
    );
    assert!(calls("list_models") >= 18, "{}", calls("list_models"));
    assert!(fixture.cases.len() >= 377, "the fixture lost cases");
}
