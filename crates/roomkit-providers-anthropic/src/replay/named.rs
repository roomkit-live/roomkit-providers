//! The cases the provider answers otherwise than Python: each with the
//! reason, how its replay then differs from Python's, and exactly what the
//! provider does instead. The replay asserts both: it differs in what the
//! entry states and in nothing else, and the provider sends as many
//! requests and gives what the entry states.

use roomkit_providers_client::harness::named::{Does, Entry, Instead, Named, Says, fails};

use super::fixture;

/// What the answer of most cases says.
const SAID: &str = "Hello there.";

/// The usage of an answer whose counts all read.
const COUNTED: &str = r#"{"input_tokens":12,"output_tokens":5}"#;

/// An answer after one request, whole or streamed, that says `text`, stops
/// for `finish` and counts this usage.
const fn answers(text: &'static str, finish: &'static str, usage: &'static str) -> Instead {
    let does = Does::Answers(text, finish, usage);
    Instead { requests: 1, does }
}

/// The answer the cases of a usage give, with this usage.
const fn counts(usage: &'static str) -> Instead {
    answers(SAID, "end_turn", usage)
}

/// A failure after one request with this status, in these words, that
/// says nothing of the context.
const fn refused(status: u16, retryable: bool, says: &'static str) -> Instead {
    fails(Some(status), retryable, says)
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
/// The events differ from Python's, whose stream ends without a failure.
const EVENTS_AND_AN_ERROR: &[&str] = &["the events: ", "a failure where Python answers: "];
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
/// The failure says the context overflowed, where Python's leaves it unsaid.
const AN_OVERFLOW: &[&str] = &["the failure's context_overflow differs: "];

const REDIRECT: &str = "A redirect is not followed: the maintainer's decision. Python's SDK \
     follows it, a 301, a 302 or a 303 as a GET without the request's body, and sends the key \
     to the host it names.";

/// The failure of a redirect of `status`, after the one request.
const fn not_followed(status: u16, says: &'static str) -> Instead {
    refused(status, false, says)
}

const NOT_FOLLOWED_301: &str = "the endpoint answered with a redirect (301), which is not followed";
const NOT_FOLLOWED_302: &str = "the endpoint answered with a redirect (302), which is not followed";
const NOT_FOLLOWED_303: &str = "the endpoint answered with a redirect (303), which is not followed";
const NOT_FOLLOWED_307: &str = "the endpoint answered with a redirect (307), which is not followed";
const NOT_FOLLOWED_308: &str = "the endpoint answered with a redirect (308), which is not followed";

const CUT_SHORT: &str = "A stream that ends with neither a stop reason nor the event that ends \
     its message was cut: what came is handed on, a call whose block closed among it, then it \
     fails, and a call still open is never handed. The maintainer's decision. Python reads it as \
     complete.";
const CUT_SHORT_WORDS: &str =
    "the provider's stream ended with neither a stop reason nor the end of its message";
const COUNT: &str =
    "A count that is no JSON whole number stays unknown: it is neither converted nor fatal.";
const ERROR_OBJECT: &str = "A JSON error object that answers a request with a 200 reads as the \
     status its type names, as the same error told as an event of a stream does: the \
     maintainer's decision. Python says the stream carried no event, a final failure that \
     names no status.";
const MARK: &str = "A byte order mark that starts a stream is ignored, as the format of a stream \
     of events says, and the first event is read: the maintainer's decision. Python's SDK reads \
     the line the mark starts as no event, and fails on the event after it.";
const KEY_AS_WRITTEN: &str = "A key of the request goes in its header as the request wrote it. \
     White space around a header's value is no part of it: a server reads a blank key as an \
     empty one, and a key in white space without it. Python hands the key to its HTTP client, \
     which refuses such a value: on a wire it sends nothing, and fails as a lost connection, \
     worth a retry.";
const CALL_ARGUMENTS: &str = "A call whose arguments are no JSON object is handed as the \
     contract reads it, blank arguments as none and unreadable ones as a call that does not \
     run. Python's SDK fails the whole stream on them.";

/// The questioned cases the provider answers otherwise than Python: the
/// reason, and what it does instead.
const QUESTIONED_OTHERWISE: &[Entry] = &[
    (
        "base_url_with_a_query",
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
        "base_url_that_is_empty",
        "An empty base URL is no URL, and is refused when the provider is built. Python's SDK \
         reads it as none and asks Anthropic's own endpoint, without the profile of its model \
         and without the check of its tool names.",
        &["the provider is not built: "],
        Instead {
            requests: 0,
            does: Does::Unbuilt(
                "the provider cannot be built: its base URL is not an http or https URL",
            ),
        },
    ),
    (
        "key_of_the_request_that_is_blank",
        KEY_AS_WRITTEN,
        &[r#"request 0: x-api-key is Some("")"#],
        answers(SAID, "end_turn", COUNTED),
    ),
    (
        "key_of_the_request_in_white_space",
        KEY_AS_WRITTEN,
        &[r#"request 0: x-api-key is Some("placeholder-not-a-key-in-spaces")"#],
        answers(SAID, "end_turn", COUNTED),
    ),
    (
        "usage_counts_written_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_output_written_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_below_zero",
        COUNT,
        ANOTHER_RESPONSE,
        counts("{}"),
    ),
    (
        "usage_counts_that_are_true",
        COUNT,
        ANOTHER_RESPONSE,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_a_cache_counter_written_as_text",
        COUNT,
        ANOTHER_RESPONSE,
        counts(COUNTED),
    ),
    (
        "stream_usage_counts_written_as_text",
        COUNT,
        OTHER_EVENTS,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "answer_200_that_is_an_error_object",
        ERROR_OBJECT,
        ANOTHER_FAILURE,
        refused(529, true, "Overloaded"),
    ),
    (
        "stream_answer_200_that_is_an_error_object",
        ERROR_OBJECT,
        ANOTHER_FAILURE,
        refused(429, true, "This request would exceed the rate limit."),
    ),
    (
        "stream_that_ends_before_its_stop_reason",
        CUT_SHORT,
        EVENTS_AND_AN_ERROR,
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_that_ends_inside_a_text",
        CUT_SHORT,
        EVENTS_AND_AN_ERROR,
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "answer_that_ends_inside_a_text",
        CUT_SHORT,
        &["a failure where Python answers: "],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_of_a_message_start_alone",
        CUT_SHORT,
        EVENTS_AND_AN_ERROR,
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_tool_call_that_ends_without_an_ending",
        CUT_SHORT,
        // The call as it was composed, never the whole call, to run.
        &[
            r#"the events: [{"type":"usage","usage":{"input_tokens":12}},{"type":"tool_call_delta","id":"toolu_1","name":"lookup_order","index":0,"arguments_delta":""},{"type":"tool_call_delta","id":"toolu_1","name":"lookup_order","index":0,"arguments_delta":"{\"number\": \"A-1\"}"}]"#,
            "a failure where Python answers: ",
        ],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_that_ends_inside_an_event",
        CUT_SHORT,
        // The text that came before the event the body ends inside.
        &[
            r#"the events: [{"type":"usage","usage":{"input_tokens":12}},{"type":"text_delta","text":"Hel"}]"#,
            "a failure where Python answers: ",
        ],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_tool_call_closed_before_the_stream_ends",
        CUT_SHORT,
        // The call, whose block closed, as it came, and no done event.
        &[
            r#"the events: [{"type":"usage","usage":{"input_tokens":12}},{"type":"tool_call_delta","id":"toolu_1","name":"lookup_order","index":0,"arguments_delta":""},{"type":"tool_call_delta","id":"toolu_1","name":"lookup_order","index":0,"arguments_delta":"{\"number\": \"A-1\"}"},{"type":"tool_call","id":"toolu_1","name":"lookup_order","arguments":{"number":"A-1"},"metadata":{},"served":null,"partial":false,"garbled":false,"intent":null}]"#,
            "a failure where Python answers: ",
        ],
        fails(None, true, CUT_SHORT_WORDS),
    ),
    (
        "stream_last_event_without_its_blank_line",
        "An event the answer ends on with no blank line after it is read when its data came \
         whole: the maintainer's decision. Python's SDK drops it, with the stop reason and the \
         usage it holds.",
        OTHER_EVENTS,
        counts(COUNTED),
    ),
    (
        "stream_tool_call_whose_arguments_are_blank",
        CALL_ARGUMENTS,
        EVENTS_NOT_AN_ERROR,
        answers("", "tool_use", COUNTED),
    ),
    (
        "stream_tool_call_whose_arguments_are_no_json",
        CALL_ARGUMENTS,
        EVENTS_NOT_AN_ERROR,
        answers("", "tool_use", COUNTED),
    ),
    (
        "tool_call_whose_arguments_are_no_json",
        CALL_ARGUMENTS,
        A_RESPONSE_NOT_AN_ERROR,
        answers("", "tool_use", COUNTED),
    ),
    (
        "stream_tool_calls_that_share_an_id_and_are_left_open",
        "A call left open is handed once its message is over, under the id its composition \
         told, whatever id an earlier call carried: here as a call the response cut. Python \
         tells its composition and never hands it.",
        OTHER_EVENTS,
        answers("", "max_tokens", COUNTED),
    ),
    (
        "stream_tool_call_without_an_id",
        "A call is handed once, when its block closes, under the one id the provider gave it. \
         Python hands a call whose block names no id twice, under two ids, and it would run \
         twice.",
        OTHER_EVENTS,
        answers("", "tool_use", COUNTED),
    ),
    (
        "redirect_301",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(301, NOT_FOLLOWED_301),
    ),
    (
        "redirect_302",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(302, NOT_FOLLOWED_302),
    ),
    (
        "redirect_303",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(303, NOT_FOLLOWED_303),
    ),
    (
        "redirect_307",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(307, NOT_FOLLOWED_307),
    ),
    (
        "redirect_308",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(308, NOT_FOLLOWED_308),
    ),
    (
        "redirect_307_to_another_host",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(307, NOT_FOLLOWED_307),
    ),
    (
        "redirect_302_to_another_host",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(302, NOT_FOLLOWED_302),
    ),
    (
        "redirect_from_anthropic_s_endpoint_to_another_host",
        REDIRECT,
        &[
            "1 requests sent, where Python sent 2",
            "the events: []",
            "a failure where Python answers: ",
        ],
        not_followed(308, NOT_FOLLOWED_308),
    ),
    (
        "redirect_to_a_relative_location",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(307, NOT_FOLLOWED_307),
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
        not_followed(307, NOT_FOLLOWED_307),
    ),
    (
        "list_models_redirect_to_another_host",
        REDIRECT,
        NOT_FOLLOWED,
        not_followed(302, NOT_FOLLOWED_302),
    ),
    (
        "stream_that_starts_with_a_byte_order_mark",
        MARK,
        EVENTS_NOT_AN_ERROR,
        counts(COUNTED),
    ),
    (
        "answer_that_starts_with_a_byte_order_mark",
        MARK,
        A_RESPONSE_NOT_AN_ERROR,
        counts(COUNTED),
    ),
    (
        "status_400_whose_body_starts_with_a_byte_order_mark",
        "A byte order mark before an error body is no part of it: the body is read as the JSON \
         it is, and the failure says what its error object says. Python's message is the \
         body's text, its mark included.",
        OTHER_WORDS,
        refused(400, false, "messages: at least one message is required"),
    ),
];

const RAW: &str = "Python raises a raw exception, no ProviderError. A body that is no page of \
     models with their ids is no listing.";
const SDK_S_OWN: &str = "Python's list_models wraps no error: the anthropic SDK's is raised as \
     it is. The provider fails as it does for a message.";
const NO_LISTING: Instead = fails(
    None,
    false,
    "The provider answered with a body it could not read: the model listing is no page of \
     models with their ids",
);

/// The cases whose failure no provider reproduces: the anthropic SDK's own,
/// or a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[Entry] = &[
    (
        "list_models_status_401",
        SDK_S_OWN,
        &["a AuthenticationError no provider reproduces"],
        refused(401, false, "invalid x-api-key"),
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
        "list_models_page_whose_models_are_null",
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
        "list_models_entry_whose_id_is_a_number",
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
    (
        "list_models_answered_with_an_error_object",
        "Python raises a raw TypeError. An error object in place of the listing fails as the \
         status its type names.",
        &["a TypeError no provider reproduces"],
        refused(529, true, "Overloaded"),
    ),
];

const NO_ECHO: &str = "A body that is no JSON is not echoed into the failure: the message is \
     the status's reason. Python's message is the body.";
const OVERFLOW: &str = "The failure says the context overflowed: the whole body is read for it, \
     whatever part of it the failure's text repeats. Python leaves the question open, for a \
     reader of its message, which holds the body.";
const STREAM_COUNT: &str = "Python fails on a count that is no whole number, or that the end of \
     the message leaves out. The count stays unknown, and the answer is given.";
const NO_USAGE: &str = "Python's SDK fails on a message whose start or end tells no usage. The \
     counts stay unknown, and the answer is given.";
const TOO_LONG: &str = "prompt is too long: 210000 tokens > 200000 maximum";
const EVENT_LINE: &str = "An event is read by the type its data names: the reader of a stream's \
     events gives the data of each, not the line that names it. Python's SDK reads an event by \
     its event line.";

/// A failure after one request that says the context overflowed.
const fn overflowed(says: &'static str) -> Instead {
    let does = Does::Fails(Some(400), false, Some(true), Says::Exactly(says));
    Instead { requests: 1, does }
}

/// The cases the provider answers otherwise by a rule of its own, where the
/// fixture questions nothing and Python fails with a `ProviderError` or
/// answers: the reason, and what the provider does instead.
const BY_ITS_OWN_RULE: &[Entry] = &[
    (
        "status_502_with_a_page",
        NO_ECHO,
        OTHER_WORDS,
        refused(502, true, "the provider answered HTTP 502: Bad Gateway"),
    ),
    (
        "status_400_with_a_plain_text",
        NO_ECHO,
        OTHER_WORDS,
        refused(400, false, "the provider answered HTTP 400: Bad Request"),
    ),
    (
        "context_overflow_by_its_words",
        OVERFLOW,
        AN_OVERFLOW,
        overflowed(TOO_LONG),
    ),
    (
        "stream_context_overflow",
        OVERFLOW,
        AN_OVERFLOW,
        overflowed(TOO_LONG),
    ),
    (
        "status_400_with_a_body_of_another_shape",
        "The failure says the context overflowed: the whole body is read for it, here a body \
         of another shape than Anthropic's, of which the failure's text repeats nothing. \
         Python leaves the question open.",
        AN_OVERFLOW,
        overflowed("the provider answered HTTP 400: Bad Request"),
    ),
    (
        "usage_counts_that_are_words",
        STREAM_COUNT,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_output_that_is_a_word",
        STREAM_COUNT,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_with_a_fraction",
        STREAM_COUNT,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_output_with_a_fraction",
        STREAM_COUNT,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_no_output_at_the_end",
        STREAM_COUNT,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_no_usage_at_the_start",
        NO_USAGE,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_no_usage_at_the_end",
        NO_USAGE,
        A_RESPONSE_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_that_are_words",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_usage_output_that_is_a_word",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_with_a_fraction",
        STREAM_COUNT,
        EVENTS_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_usage_no_usage_at_the_start",
        NO_USAGE,
        EVENTS_NOT_AN_ERROR,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_events_named_by_their_event_line_alone",
        EVENT_LINE,
        // What the message's start told, then the failure of the first
        // event whose data names no type.
        &[
            r#"the events: [{"type":"usage","usage":{"input_tokens":12}}]"#,
            "a failure where Python answers: ",
        ],
        fails(
            None,
            false,
            "The provider answered with a body it could not read: an event of the stream names \
             no type",
        ),
    ),
    (
        "stream_events_without_their_event_line",
        EVENT_LINE,
        EVENTS_NOT_AN_ERROR,
        answers("Hello.", "end_turn", COUNTED),
    ),
    (
        "stream_error_event_that_is_no_json",
        "An event whose data is no JSON fails the stream in the reader's words, which repeat \
         nothing of it: the reader of a stream's events gives no event line, so that an error \
         event is known by its data alone. Python's message is the event's text.",
        OTHER_WORDS,
        Instead {
            requests: 1,
            does: Does::Fails(None, false, None, Says::Holding("expected value at line 1")),
        },
    ),
    (
        "redirect_without_a_location",
        "A redirect is not followed, and its failure carries its status. Python's SDK raises \
         one that names no location as a status that is no success, and its failure carries \
         none.",
        &["the failure's status_code differs: "],
        not_followed(307, NOT_FOLLOWED_307),
    ),
    (
        "turn_whose_role_the_provider_does_not_know",
        "The contract reads the roles system, user, assistant and tool, and refuses a request \
         that holds another before any provider sees it. Python sends the role as it is \
         written.",
        &["the request is not read: "],
        Instead {
            requests: 0,
            does: Does::Unread(Says::Holding("expected one of the roles")),
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
const NOTES: &str = "The cases of python_messages_cases.json the provider answers otherwise \
                     than Python, as src/replay/named.rs states them, for a replay outside the \
                     crate. 'differs' lists the differences from Python a replay finds, each by \
                     the words it starts with; 'requests' is how many requests the provider \
                     sends; 'does' is what the call gives. A test of the crate keeps this file \
                     equal to the source: run that test with NAMED_CASES_OUT naming the file to \
                     rewrite it.";

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
    assert!(
        held == stated,
        "rewrite tests/fixtures/named_cases.json (NAMED_CASES_OUT), and commit the file"
    );
}

/// Each named case is a case of the fixture, in the list its case belongs
/// to: questioned there, failing with what no provider reproduces, or
/// neither; and it says why.
#[test]
fn each_named_case_is_one_the_fixture_sets_apart() {
    NAMED.set_apart(&fixture());
}
