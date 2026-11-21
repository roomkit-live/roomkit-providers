//! The wire cases captured on `RoomKit`'s Python `OpenAI` provider, read
//! with the contract's types and this crate's configuration:
//! `tests/fixtures/python_chat_cases.json`, written by
//! `scripts/generate_openai_chat_cases.py` (`make python-openai-chat-cases`).
//!
//! Each case holds a configuration, a request, the HTTP exchanges Python had
//! with a fake of the endpoint, and what Python returned. The provider
//! replays them in its own tests; this test holds the table to its shape,
//! reads every part of it, and requires every kind of case.
//!
//! What every fixture of recorded cases holds is checked by the harness of
//! `roomkit-providers-client` (`harness::shape`), which says how a body, a
//! failure and a questioned case are written. What is `OpenAI`'s own is
//! checked here: its configuration, its catalogue, the URLs it asks, and
//! the cases the fixture must hold.

use std::collections::BTreeSet;

use roomkit_providers_client::harness::shape::{Case, Fixture, Shape};
use roomkit_providers_openai::OpenAiConfig;
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_chat_cases.json");

/// The key of every configuration, and the key a request carries.
const KEY: &str = "placeholder-not-a-key";
const REQUEST_KEY: &str = "placeholder-not-a-key-either";

/// `OpenAI`'s own endpoint, which a configuration with no base URL calls.
const OPENAI: &str = "https://api.openai.com/v1";

/// What the openai SDK says of a failure that has no answer, or of an error
/// event that holds no message.
const SDK_WORDS: [&str; 3] = [
    "Connection error.",
    "Request timed out.",
    "An error occurred during streaming",
];

/// The classes of the openai SDK a call may raise as they are.
const SDK_ERRORS: [&str; 12] = [
    "OpenAIError",
    "APIConnectionError",
    "APITimeoutError",
    "APIStatusError",
    "BadRequestError",
    "AuthenticationError",
    "PermissionDeniedError",
    "NotFoundError",
    "ConflictError",
    "UnprocessableEntityError",
    "RateLimitError",
    "InternalServerError",
];

/// The cases that pin a behaviour of Python that is not settled: reported
/// upstream as a possible defect, or a point where the Rust provider is
/// planned otherwise and the maintainer decides. A replay may set them
/// aside, or expect otherwise once each is settled: a case joins or leaves
/// this list with that decision.
const QUESTIONED: [&str; 41] = [
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
    "stream_call_name_after_its_arguments",
    "stream_empty_finish_reason_without_an_end_mark",
    "stream_last_event_without_its_blank_line",
    "stream_last_event_without_its_line_end",
    "stream_status_400_whose_body_is_lost",
    "stream_that_ends_without_an_ending",
    "stream_that_starts_with_a_byte_order_mark",
    "stream_tool_call_empty_finish_reason_without_an_end_mark",
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
const KINDS: [&str; 83] = [
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
    "finish_empty",
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
    "detail_list",
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
    // The model listing.
    "listing",
    // What the provider says of its model, and a base URL's credentials.
    "vision",
    "base_url_credentials",
];

/// What the fixture states of itself, beside what every fixture holds.
const SHAPE: Shape<'static> = Shape {
    provider: "openai",
    sdk_version: "openai_version",
    environment: "OPENAI_*",
    notes: &[
        "environment",
        "provenance",
        "exchanges.request",
        "exchanges.response",
        "supports_vision",
        "compare",
        "questioned",
        "sdk",
    ],
    own_members: &[],
    key: KEY,
    request_keys: &[REQUEST_KEY],
    unread: &[],
    sdk_errors: &SDK_ERRORS,
    sdk_words: &SDK_WORDS,
    sdk_causes: &[],
    sdk_sentences: &[],
    raises_an_event_s_data: false,
    longest_wait: 120.0,
};

fn fixture() -> Fixture {
    Fixture::read(FIXTURE, &SHAPE)
}

#[test]
fn the_fixture_says_where_it_comes_from_and_how_it_is_written() {
    fixture().says_where_it_comes_from(&SHAPE);
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

/// Each case says whether its provider's model takes images, unless no
/// provider was built; the fixture shows a model that takes some and a
/// model that takes none, on OpenAI's own endpoint and behind a base URL.
#[test]
fn a_case_says_whether_its_model_takes_images() {
    let mut said = BTreeSet::new();
    for case in fixture().cases {
        let unbuilt = case.expected.error.as_ref();
        let unbuilt = unbuilt.is_some_and(|error| error.class == "OpenAIError");
        assert_eq!(case.supports_vision.is_none(), unbuilt, "{}", case.name);
        let own = case.config.get("base_url").is_none();
        said.extend(case.supports_vision.map(|sees| (own, sees)));
    }
    let shown: Vec<(bool, bool)> = said.into_iter().collect();
    assert_eq!(shown, [(false, true), (true, false), (true, true)]);
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
    fixture().the_timeouts_are_the_ones_python_sent(|config| {
        let config = serde_json::from_value::<OpenAiConfig>(config.clone()).ok()?;
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
        failures.compared >= 45,
        "{} messages compared",
        failures.compared
    );
    assert!(
        failures.of_a_schema >= 15,
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

/// The URL the first request of `case` goes to: its base URL, `OpenAI`'s
/// own when none is given, then the path of Chat Completions, or of the
/// model listing, after the one slash that ends the base URL.
fn asked_at(case: &Case) -> String {
    let base = case.config.get("base_url").and_then(Value::as_str);
    let base = base.unwrap_or(OPENAI);
    let path = if case.call == "list_models" {
        "models"
    } else {
        "chat/completions"
    };
    if case.name == MANGLED.0 {
        assert!(case.questioned.is_some(), "{}", case.name);
        return MANGLED.1.to_owned();
    }
    format!("{}/{path}", base.strip_suffix('/').unwrap_or(base))
}

/// Each request went where its configuration says: the base URL, `OpenAI`'s
/// own when none is given, then the path of Chat Completions, or of the
/// model listing; or where the answer before it redirected it.
#[test]
fn each_request_was_sent_to_the_url_its_case_expects() {
    let fixture = fixture();
    fixture.each_request_was_sent_where_its_case_expects(asked_at);
    // A case with no base URL shows OpenAI's own endpoint.
    for case in &fixture.cases {
        if case.config.get("base_url").is_none() {
            for exchange in &case.exchanges {
                assert!(exchange.request.url.starts_with(OPENAI), "{}", case.name);
            }
        }
    }
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
    fixture().each_minted_call_id_has_python_s_shape(Some("minted_call_id"));
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
    assert!(calls("generate") >= 211, "{}", calls("generate"));
    assert!(
        calls("generate_stream") >= 4,
        "{}",
        calls("generate_stream")
    );
    assert!(
        calls("generate_structured_stream") >= 115,
        "{}",
        calls("generate_structured_stream")
    );
    assert!(calls("list_models") >= 10, "{}", calls("list_models"));
    assert!(fixture.cases.len() >= 340, "the fixture lost cases");
}
