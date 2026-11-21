//! The cases the provider answers otherwise than Python: each with the
//! reason, how its replay then differs from Python's, and exactly what the
//! provider does instead. The replay asserts both: it differs in what the
//! entry states and in nothing else, and the provider sends as many
//! requests and gives what the entry states.

use roomkit_providers_client::harness::named::{Does, Entry, Instead, Named, Says, fails};

use super::fixture;

/// A whole answer after one request, the one the cases of a usage give,
/// with this usage.
const fn counts(usage: &'static str) -> Instead {
    let does = Does::Answers("Hello there.", "stop", usage);
    Instead { requests: 1, does }
}

/// A stream after one request, which says this, stops for `stop` and
/// counts this usage.
const fn streams(text: &'static str, usage: &'static str) -> Instead {
    let does = Does::Answers(text, "stop", usage);
    Instead { requests: 1, does }
}

/// A stream after one request, the one the cases of a usage give, with
/// this usage.
const fn stream_counts(usage: &'static str) -> Instead {
    streams("Hello.", usage)
}

/// The response differs from Python's.
const ANOTHER_RESPONSE: &[&str] = &["the response: "];
/// A response, where Python raises.
const A_RESPONSE_NOT_AN_ERROR: &[&str] = &[
    "the response where Python gives none: ",
    "an answer where Python fails: ",
];
/// The events differ from Python's.
const OTHER_EVENTS: &[&str] = &["the events: "];
/// The events differ from Python's, whose stream fails.
const EVENTS_NOT_AN_ERROR: &[&str] = &["the events: ", "an answer where Python fails: "];
/// One request and a failure, where Python follows to a second request
/// and answers.
const NOT_FOLLOWED: &[&str] = &[
    "1 requests sent, where Python sent 2",
    "a failure where Python answers: ",
];
/// The failure's words are the provider's, where Python's hold the body.
const OTHER_WORDS: &[&str] = &["the failure says "];

/// The failure differs from Python's in its status, in whether it is
/// retried and in its words.
const ANOTHER_FAILURE: &[&str] = &[
    "the failure's status_code differs: ",
    "the failure's retryable differs: ",
    "the failure says ",
];

const REDIRECT: &str = "A redirect is not followed: the maintainer's decision.";

/// The failure of a redirect of `status`, after the one request.
const fn not_followed(status: u16, says: &'static str) -> Instead {
    fails(Some(status), false, says)
}

const NO_COMPLETION: &str = "A 200 whose body is JSON and no completion is a failure: the \
     maintainer's decision. Python reads it as an answer that says nothing.";
const CUT_SHORT: &str = "A stream that ends with neither a finish reason nor its end mark was \
     cut: what came is handed on, then it fails, and a call it brought is never handed whole. \
     The maintainer's decision. Python reads it as complete.";
const CUT_SHORT_WORDS: &str =
    "the provider's stream ended with neither a finish reason nor its end mark";
const COUNT: &str =
    "A count that is no JSON whole number stays unknown: it is neither converted nor fatal.";
const LAST_EVENT: &str = "An event the answer ends on with no blank line after it is read: \
     the maintainer's decision. Python's client drops it.";
const COUNTED: &str = r#"{"input_tokens":1200,"output_tokens":45}"#;

/// The questioned cases the provider answers otherwise than Python: the
/// reason, and what it does instead.
const QUESTIONED_OTHERWISE: &[Entry] = &[
    (
        "base_url_with_a_trailing_slash_and_a_query",
        "A base URL with a query is refused when the provider is built: the maintainer's \
         decision. Python sends to a path written after the query.",
        &["the provider is not built: "],
        Instead {
            requests: 0,
            does: Does::Unbuilt(
                "the provider cannot be built: its base URL has a query or a fragment, which \
                 the path of the API cannot follow",
            ),
        },
    ),
    (
        "redirect",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(
            307,
            "the endpoint answered with a redirect (307), which is not followed",
        ),
    ),
    (
        "redirect_301",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(
            301,
            "the endpoint answered with a redirect (301), which is not followed",
        ),
    ),
    (
        "redirect_302_to_another_host",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(
            302,
            "the endpoint answered with a redirect (302), which is not followed",
        ),
    ),
    (
        "redirect_303",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(
            303,
            "the endpoint answered with a redirect (303), which is not followed",
        ),
    ),
    (
        "redirect_308",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(
            308,
            "the endpoint answered with a redirect (308), which is not followed",
        ),
    ),
    (
        "redirects_past_the_hop_limit",
        REDIRECT,
        // Python follows twenty times, then fails with no status, a
        // failure worth a retry.
        &[
            "1 requests sent, where Python sent 21",
            "the failure's status_code differs: ",
            "the failure's retryable differs: ",
        ],
        not_followed(
            307,
            "the endpoint answered with a redirect (307), which is not followed",
        ),
    ),
    (
        "answer_200_that_is_no_completion",
        NO_COMPLETION,
        &["a failure where Python answers: "],
        fails(
            None,
            false,
            "The provider answered with a body it could not read: not a chat completion",
        ),
    ),
    (
        "stream_that_ends_without_an_ending",
        CUT_SHORT,
        // The text that came, with no done event after it.
        &[
            r#"the events: [{"type":"text_delta","text":"Hello."}]"#,
            "a failure where Python answers: ",
        ],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_tool_call_that_ends_without_an_ending",
        CUT_SHORT,
        // The call as it was written, never the whole call, to run.
        &[
            r#"the events: [{"type":"tool_call_delta","id":"call_1","name":"lookup_order","index":0,"arguments_delta":"{\"number\": \"A-1\"}"}]"#,
            "a failure where Python answers: ",
        ],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_answered_with_a_json_error_object",
        "A JSON error object that answers a stream request with a 200 reads as the status it \
         names, as generate reads the same body: the maintainer's decision. Python says the \
         stream carried no event, a final failure that names no status.",
        ANOTHER_FAILURE,
        Instead {
            requests: 1,
            does: Does::Fails(
                Some(529),
                true,
                None,
                Says::Exactly("The engine is currently overloaded."),
            ),
        },
    ),
    (
        "stream_that_starts_with_a_byte_order_mark",
        "A byte order mark that starts a stream is ignored, as the format of a stream of \
         events says, and the first event is read: the maintainer's decision. Python's \
         client reads the line the mark starts as no data.",
        OTHER_EVENTS,
        streams("Hello.", "{}"),
    ),
    (
        "usage_completion_tokens_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_that_are_true",
        COUNT,
        ANOTHER_RESPONSE,
        counts("{}"),
    ),
    (
        "usage_cached_tokens_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(COUNTED),
    ),
    (
        "usage_reasoning_tokens_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(COUNTED),
    ),
    (
        "usage_cache_write_written_as_text_in_the_details",
        COUNT,
        ANOTHER_RESPONSE,
        counts(COUNTED),
    ),
    (
        "usage_counts_below_zero",
        COUNT,
        ANOTHER_RESPONSE,
        counts("{}"),
    ),
    (
        "stream_usage_completion_tokens_as_text",
        COUNT,
        OTHER_EVENTS,
        stream_counts(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_that_are_true",
        COUNT,
        OTHER_EVENTS,
        stream_counts("{}"),
    ),
    (
        "stream_usage_cached_tokens_as_text",
        COUNT,
        OTHER_EVENTS,
        stream_counts(COUNTED),
    ),
    (
        "stream_usage_reasoning_tokens_as_text",
        COUNT,
        OTHER_EVENTS,
        stream_counts(COUNTED),
    ),
    (
        "stream_last_event_without_its_blank_line",
        LAST_EVENT,
        OTHER_EVENTS,
        streams("Hello.", "{}"),
    ),
    (
        "stream_last_event_without_its_line_end",
        LAST_EVENT,
        OTHER_EVENTS,
        streams("Hello.", "{}"),
    ),
];

const RAW: &str = "Python raises a raw exception, no ProviderError.";
const SDK_S_OWN: &str = "Python's list_models wraps no error: the openai SDK's is raised as it \
     is. The provider fails as it does for a completion.";
const NO_LISTING: Instead = fails(
    None,
    false,
    "The provider answered with a body it could not read: the model listing is no page of \
     models with their ids",
);

/// The cases whose failure no provider reproduces: the openai SDK's own, or
/// a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[Entry] = &[
    (
        "empty_key",
        "The openai SDK builds no client without a key. The provider is built, as the \
         source's, and sends no Authorization.",
        &[
            "1 requests sent, where Python sent 0",
            "a OpenAIError no provider reproduces",
        ],
        Instead {
            requests: 1,
            does: Does::AsksWithoutAKey,
        },
    ),
    (
        "choices_that_are_no_list",
        "Python raises a raw AttributeError. The body is no completion.",
        &["a AttributeError no provider reproduces"],
        fails(
            None,
            false,
            "The provider answered with a body it could not read: not a chat completion",
        ),
    ),
    (
        "usage_prompt_tokens_as_text",
        "Python raises a raw TypeError. The count stays unknown.",
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_cached_tokens_as_a_word",
        "Python raises a raw TypeError. The count stays unknown.",
        A_RESPONSE_NOT_AN_ERROR,
        counts(COUNTED),
    ),
    (
        "usage_completion_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_with_a_fraction",
        "Python raises a raw ValidationError. The counts stay unknown.",
        A_RESPONSE_NOT_AN_ERROR,
        counts("{}"),
    ),
    (
        "usage_reasoning_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        A_RESPONSE_NOT_AN_ERROR,
        counts(COUNTED),
    ),
    (
        "list_models_status_401",
        SDK_S_OWN,
        &["a AuthenticationError no provider reproduces"],
        fails(
            Some(401),
            false,
            "the provider answered HTTP 401 (invalid_request_error, invalid_api_key): Incorrect \
             API key provided.",
        ),
    ),
    (
        "list_models_connection_refused",
        SDK_S_OWN,
        &["a APIConnectionError no provider reproduces"],
        fails(None, true, "the connection to the provider failed"),
    ),
    (
        "list_models_page_without_its_models",
        RAW,
        &["a TypeError no provider reproduces"],
        NO_LISTING,
    ),
    (
        "list_models_entry_without_an_id",
        RAW,
        &["a ValidationError no provider reproduces"],
        NO_LISTING,
    ),
    (
        "list_models_answered_with_a_page",
        RAW,
        &["a AttributeError no provider reproduces"],
        NO_LISTING,
    ),
];

const NO_ECHO: &str = "A body that is no JSON is not echoed into the failure: the message is \
     the status's reason. Python's message is the body.";
const STREAM_COUNT: &str = "Python fails the stream on a count that is no whole number. The \
     count stays unknown, as in a whole answer.";

/// The cases the provider answers otherwise by a rule of its own, where the
/// fixture questions nothing and Python fails with a `ProviderError` or
/// answers: the reason, and what the provider does instead.
const BY_ITS_OWN_RULE: &[Entry] = &[
    (
        "status_502_with_a_page",
        NO_ECHO,
        OTHER_WORDS,
        fails(
            Some(502),
            true,
            "the provider answered HTTP 502: Bad Gateway",
        ),
    ),
    (
        "status_400_with_a_plain_text",
        NO_ECHO,
        OTHER_WORDS,
        fails(
            Some(400),
            false,
            "the provider answered HTTP 400: Bad Request",
        ),
    ),
    (
        "context_overflow_by_its_words",
        "The failure says the context overflowed: the whole body is read for it, whatever \
         part of it the failure's text repeats, here the words the server gives under `detail`. \
         Python leaves the question open, for a reader of its message, which holds the body.",
        &["the failure's context_overflow differs: "],
        Instead {
            requests: 1,
            does: Does::Fails(
                Some(400),
                false,
                Some(true),
                Says::Exactly(
                    "the provider answered HTTP 400: This model's maximum context length is \
                     128000 tokens. However, your messages resulted in 130000 tokens.",
                ),
            ),
        },
    ),
    (
        "stream_usage_prompt_tokens_as_text",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        stream_counts(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_usage_completion_tokens_as_a_word",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        stream_counts(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_with_a_fraction",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        stream_counts("{}"),
    ),
    (
        "stream_usage_cached_tokens_as_a_word",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        stream_counts(COUNTED),
    ),
    (
        "stream_usage_reasoning_tokens_as_a_word",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        stream_counts(COUNTED),
    ),
    (
        "stream_event_with_no_data",
        "An event that holds no data is no event, as the format of a stream of events \
         says. Python's client reads it as JSON and fails.",
        EVENTS_NOT_AN_ERROR,
        streams("Hello.", r#"{"input_tokens":12,"output_tokens":5}"#),
    ),
    (
        "stream_call_index_that_is_a_text",
        "A call's index is a whole number: another fails the stream. Python takes any \
         value as the key of the call.",
        // No event is given before the failure, where Python gives the
        // call's.
        &["the events: []", "a failure where Python answers: "],
        Instead {
            requests: 1,
            does: Does::Fails(
                None,
                false,
                None,
                Says::Holding("invalid type: string \"0\", expected u64"),
            ),
        },
    ),
];

/// The cases the provider answers otherwise than Python, each in the list
/// its case belongs to.
pub(super) const NAMED: Named = Named {
    questioned_otherwise: QUESTIONED_OTHERWISE,
    only_python: ONLY_PYTHON,
    by_its_own_rule: BY_ITS_OWN_RULE,
};

/// What the data file of the named cases says of itself.
const NOTES: &str = "The cases of python_chat_cases.json the provider answers otherwise than \
                     Python, as src/replay/named.rs states them, for a replay outside the crate. \
                     'differs' lists the differences from Python a replay finds, each by the words \
                     it starts with; 'requests' is how many requests the provider sends; 'does' \
                     is what the call gives. A test of the crate keeps this file equal to the \
                     source: rewrite it with make named-cases.";

/// The data file of the named cases is what this module states. Run with
/// `NAMED_CASES_OUT` naming a file, it writes the file instead.
#[test]
fn the_named_cases_file_is_what_this_module_states() {
    let stated = NAMED.as_data(NOTES);
    if let Some(out) = std::env::var_os("NAMED_CASES_OUT") {
        std::fs::write(out, stated).unwrap();
        return;
    }
    let held = include_str!("../../tests/fixtures/named_cases.json");
    assert!(held == stated, "run make named-cases, and commit the file");
}

/// Each named case is a case of the fixture, in the list its case belongs
/// to: questioned there, failing with what no provider reproduces, or
/// neither; and it says why.
#[test]
fn each_named_case_is_one_the_fixture_sets_apart() {
    NAMED.set_apart(&fixture());
}
