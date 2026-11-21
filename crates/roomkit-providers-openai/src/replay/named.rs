//! The cases the provider answers otherwise than Python: each with the
//! reason, and what the provider does instead, which the replay asserts.

use std::collections::BTreeMap;

use roomkit_providers_core::{ProviderError, StreamDone, StreamEvent};

use super::{Outcome, Replayed, fixture};
use crate::ported::support::text_of;

/// What the provider does where it does not do as Python.
#[derive(Clone, Copy, Debug)]
pub(super) enum Instead {
    /// It is not built: its failure says these words.
    Unbuilt(&'static str),
    /// It is built, where Python's is not.
    Built,
    /// The call fails, with this status, retried or not, in these words.
    Fails(Option<u16>, bool, &'static str),
    /// The call is answered, with this usage.
    Usage(&'static str),
    /// The stream ends without a failure: it said this text, and stopped
    /// for this reason.
    Says(&'static str, &'static str),
}

impl Instead {
    /// Whether `replayed` did what this says.
    pub(super) fn is_done(self, replayed: &Replayed) -> bool {
        let failed = |error: &ProviderError, status: Option<u16>, retryable, words| {
            (error.status_code == status || status.is_none())
                && error.retryable == retryable
                && error.message.contains(words)
        };
        match (self, &replayed.unbuilt, &replayed.outcome) {
            (Self::Unbuilt(words), Some(error), _) => {
                error.before_request && error.message.contains(words)
            }
            (Self::Built, None, _) => true,
            (
                Self::Fails(status, retryable, words),
                None,
                Some(
                    Outcome::Response(Err(error))
                    | Outcome::Models(Err(error))
                    | Outcome::Events(_, Some(error)),
                ),
            ) => failed(error, status, retryable, words),
            (Self::Usage(usage), None, Some(Outcome::Response(Ok(response)))) => {
                serde_json::to_string(&response.usage).unwrap() == usage
            }
            (Self::Usage(usage), None, Some(Outcome::Events(events, None))) => {
                let done = done_of(events);
                done.map(|done| serde_json::to_string(&done.usage).unwrap())
                    .as_deref()
                    == Some(usage)
            }
            (Self::Says(text, finish), None, Some(Outcome::Events(events, None))) => {
                let stopped = done_of(events).and_then(|done| done.finish_reason.as_deref());
                text_of(events) == text && stopped == Some(finish)
            }
            _ => false,
        }
    }
}

/// The done event of `events`.
fn done_of(events: &[StreamEvent]) -> Option<&StreamDone> {
    events.iter().find_map(|event| match event {
        StreamEvent::Done(done) => Some(done),
        StreamEvent::TextDelta(_)
        | StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::ThinkingDelta(_)
        | StreamEvent::Usage(_) => None,
    })
}

const REDIRECT: &str = "A redirect is not followed: the maintainer's decision.";
const NO_REDIRECT: Instead = Instead::Fails(None, false, "redirect");
const COUNT: &str =
    "A count that is no JSON whole number stays unknown: it is neither converted nor fatal.";
const LAST_EVENT: &str = "An event the answer ends on with no blank line after it is read: \
     the maintainer's decision. Python's client drops it.";
const COUNTED: &str = r#"{"input_tokens":1200,"output_tokens":45}"#;

/// The questioned cases the provider answers otherwise than Python: the
/// reason, and what it does instead.
const QUESTIONED_OTHERWISE: &[(&str, &str, Instead)] = &[
    (
        "base_url_with_a_trailing_slash_and_a_query",
        "A base URL with a query is refused when the provider is built: the maintainer's \
         decision. Python sends to a path written after the query.",
        Instead::Unbuilt("a query or a fragment"),
    ),
    ("redirect", REDIRECT, NO_REDIRECT),
    ("redirect_301", REDIRECT, NO_REDIRECT),
    ("redirect_302_to_another_host", REDIRECT, NO_REDIRECT),
    ("redirect_303", REDIRECT, NO_REDIRECT),
    ("redirect_308", REDIRECT, NO_REDIRECT),
    ("redirects_past_the_hop_limit", REDIRECT, NO_REDIRECT),
    (
        "usage_completion_tokens_as_text",
        COUNT,
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    ("usage_counts_that_are_true", COUNT, Instead::Usage("{}")),
    (
        "usage_cached_tokens_as_text",
        COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "usage_reasoning_tokens_as_text",
        COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "usage_cache_write_written_as_text_in_the_details",
        COUNT,
        Instead::Usage(COUNTED),
    ),
    ("usage_counts_below_zero", COUNT, Instead::Usage("{}")),
    (
        "stream_usage_completion_tokens_as_text",
        COUNT,
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_that_are_true",
        COUNT,
        Instead::Usage("{}"),
    ),
    (
        "stream_usage_cached_tokens_as_text",
        COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "stream_usage_reasoning_tokens_as_text",
        COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "stream_last_event_without_its_blank_line",
        LAST_EVENT,
        Instead::Says("Hello.", "stop"),
    ),
    (
        "stream_last_event_without_its_line_end",
        LAST_EVENT,
        Instead::Says("Hello.", "stop"),
    ),
];

const RAW: &str = "Python raises a raw exception, no ProviderError.";
const SDK_S_OWN: &str = "Python's list_models wraps no error: the openai SDK's is raised as it \
     is. The provider fails as it does for a completion.";
const NO_LISTING: Instead = Instead::Fails(None, false, "no page of models");

/// The cases whose failure no provider reproduces: the openai SDK's own, or
/// a raw Python exception. What the provider does instead.
const ONLY_PYTHON: &[(&str, &str, Instead)] = &[
    (
        "empty_key",
        "The openai SDK builds no client without a key. The provider is built, as the \
         source's, and sends no Authorization.",
        Instead::Built,
    ),
    (
        "choices_that_are_no_list",
        "Python raises a raw AttributeError. The body is no completion.",
        Instead::Fails(None, false, "not a chat completion"),
    ),
    (
        "usage_prompt_tokens_as_text",
        "Python raises a raw TypeError. The count stays unknown.",
        Instead::Usage(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_cached_tokens_as_a_word",
        "Python raises a raw TypeError. The count stays unknown.",
        Instead::Usage(COUNTED),
    ),
    (
        "usage_completion_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_with_a_fraction",
        "Python raises a raw ValidationError. The counts stay unknown.",
        Instead::Usage("{}"),
    ),
    (
        "usage_reasoning_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        Instead::Usage(COUNTED),
    ),
    (
        "list_models_status_401",
        SDK_S_OWN,
        Instead::Fails(Some(401), false, "Incorrect API key provided."),
    ),
    (
        "list_models_connection_refused",
        SDK_S_OWN,
        Instead::Fails(None, true, "the connection to the provider failed"),
    ),
    ("list_models_page_without_its_models", RAW, NO_LISTING),
    ("list_models_entry_without_an_id", RAW, NO_LISTING),
    ("list_models_answered_with_a_page", RAW, NO_LISTING),
];

const NO_ECHO: &str = "A body that is no JSON is not echoed into the failure: the message is \
     the status's reason. Python's message is the body.";
const STREAM_COUNT: &str = "Python fails the stream on a count that is no whole number. The \
     count stays unknown, as in a whole answer.";

/// The cases the provider answers otherwise by a rule of its own, where the
/// fixture questions nothing and Python fails with a `ProviderError` or
/// answers: the reason, and what the provider does instead.
const BY_ITS_OWN_RULE: &[(&str, &str, Instead)] = &[
    (
        "status_502_with_a_page",
        NO_ECHO,
        Instead::Fails(
            Some(502),
            true,
            "the provider answered HTTP 502: Bad Gateway",
        ),
    ),
    (
        "status_400_with_a_plain_text",
        NO_ECHO,
        Instead::Fails(
            Some(400),
            false,
            "the provider answered HTTP 400: Bad Request",
        ),
    ),
    (
        "stream_usage_prompt_tokens_as_text",
        STREAM_COUNT,
        Instead::Usage(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_usage_completion_tokens_as_a_word",
        STREAM_COUNT,
        Instead::Usage(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_with_a_fraction",
        STREAM_COUNT,
        Instead::Usage("{}"),
    ),
    (
        "stream_usage_cached_tokens_as_a_word",
        STREAM_COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "stream_usage_reasoning_tokens_as_a_word",
        STREAM_COUNT,
        Instead::Usage(COUNTED),
    ),
    (
        "stream_event_with_no_data",
        "An event that holds no data is no event, as the format of a stream of events \
         says. Python's client reads it as JSON and fails.",
        Instead::Says("Hello.", "stop"),
    ),
    (
        "stream_call_index_that_is_a_text",
        "A call's index is a whole number: another fails the stream. Python takes any \
         value as the key of the call.",
        Instead::Fails(None, false, "invalid type"),
    ),
];

/// The cases the provider answers otherwise than Python, by name.
pub(super) fn named() -> BTreeMap<&'static str, Instead> {
    let lists = [QUESTIONED_OTHERWISE, ONLY_PYTHON, BY_ITS_OWN_RULE];
    let named = lists.into_iter().flatten();
    named.map(|(name, _, instead)| (*name, *instead)).collect()
}

/// Each named case is a case of the fixture, in the list its case belongs
/// to: questioned there, failing with what no provider reproduces, or
/// neither; and it says why.
#[test]
fn each_named_case_is_one_the_fixture_sets_apart() {
    let fixture = fixture();
    let case = |name: &str| {
        let found = fixture.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("no case {name}"))
    };
    let raw = |name: &str| {
        let compare = case(name).compare.as_ref();
        compare.is_some_and(|compare| compare.exactly.is_empty())
    };
    for (name, reason, _) in QUESTIONED_OTHERWISE {
        assert!(case(name).questioned.is_some(), "{name}");
        assert!(reason.ends_with('.'), "{name}");
    }
    for (name, reason, _) in ONLY_PYTHON {
        assert!(raw(name), "{name}");
        assert!(reason.ends_with('.'), "{name}");
    }
    for (name, reason, _) in BY_ITS_OWN_RULE {
        assert!(case(name).questioned.is_none() && !raw(name), "{name}");
        assert!(reason.ends_with('.'), "{name}");
    }
    let listed = QUESTIONED_OTHERWISE.len() + ONLY_PYTHON.len() + BY_ITS_OWN_RULE.len();
    assert_eq!(named().len(), listed);
}
