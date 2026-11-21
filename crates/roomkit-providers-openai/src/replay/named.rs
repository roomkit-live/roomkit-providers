//! The cases the provider answers otherwise than Python: each with the
//! reason, and exactly what the provider does instead, which the replay
//! asserts: how many requests it sends, and what the call gives.

use std::collections::BTreeMap;

use roomkit_providers_core::{ProviderError, StreamDone, StreamEvent};

use super::{Outcome, Replayed, fixture};
use crate::ported::support::text_of;

/// What a failure says.
#[derive(Clone, Copy, Debug)]
enum Says {
    /// These words, the provider's own, and no other.
    Exactly(&'static str),
    /// A reader's words, which hold these.
    Holding(&'static str),
}

/// What the provider does where it does not do as Python.
#[derive(Clone, Copy, Debug)]
enum Does {
    /// It is not built, and says why.
    Unbuilt(&'static str),
    /// The call fails: with this status, retried or not, saying whether
    /// the context overflowed, in these words.
    Fails(Option<u16>, bool, Option<bool>, Says),
    /// The call is answered, with this usage.
    Usage(&'static str),
    /// The stream ends without a failure: it said this text, and stopped
    /// for this reason.
    Stream(&'static str, &'static str),
    /// It is built where Python's is not, and asks with no `Authorization`:
    /// the fake, which the case gives no answer, answers its 599.
    AsksWithoutAKey,
}

/// What the provider does of a named case: how many requests it sends, and
/// what the call gives.
#[derive(Clone, Copy, Debug)]
pub(super) struct Instead {
    requests: usize,
    does: Does,
}

/// A failure after one request, final or not, in the provider's words.
const fn fails(status: Option<u16>, retryable: bool, says: &'static str) -> Instead {
    let does = Does::Fails(status, retryable, None, Says::Exactly(says));
    Instead { requests: 1, does }
}

/// An answer after one request, with this usage.
const fn counts(usage: &'static str) -> Instead {
    let does = Does::Usage(usage);
    Instead { requests: 1, does }
}

/// A stream after one request, which says this and stops for this reason.
const fn streams(text: &'static str, finish: &'static str) -> Instead {
    let does = Does::Stream(text, finish);
    Instead { requests: 1, does }
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

impl Instead {
    /// Whether `replayed` did exactly what this says.
    pub(super) fn is_done(self, replayed: &Replayed) -> bool {
        if replayed.seen.len() != self.requests {
            return false;
        }
        let failed = |error: &ProviderError, expected: Does| {
            let Does::Fails(status, retryable, overflow, says) = expected else {
                return false;
            };
            let said = match says {
                Says::Exactly(words) => error.message == words,
                Says::Holding(words) => error.message.contains(words),
            };
            said && error.status_code == status
                && error.retryable == retryable
                && error.context_overflow == overflow
                && !error.before_request
        };
        match (self.does, &replayed.unbuilt, &replayed.outcome) {
            (Does::Unbuilt(words), Some(error), None) => {
                error.before_request && !error.retryable && error.message == words
            }
            (
                Does::Fails(..),
                None,
                Some(
                    Outcome::Response(Err(error))
                    | Outcome::Models(Err(error))
                    | Outcome::Events(_, Some(error)),
                ),
            ) => failed(error, self.does),
            (Does::AsksWithoutAKey, None, Some(Outcome::Response(Err(error)))) => {
                let asked = replayed.seen.first();
                let no_key = asked.is_some_and(|seen| !seen.headers.contains_key("authorization"));
                let nothing = Says::Exactly("the provider answered HTTP 599: no message");
                no_key && failed(error, Does::Fails(Some(599), true, None, nothing))
            }
            (Does::Usage(usage), None, Some(Outcome::Response(Ok(response)))) => {
                serde_json::to_string(&response.usage).unwrap() == usage
            }
            (Does::Usage(usage), None, Some(Outcome::Events(events, None))) => {
                let done = done_of(events);
                let counted = done.map(|done| serde_json::to_string(&done.usage).unwrap());
                counted.as_deref() == Some(usage)
            }
            (Does::Stream(text, finish), None, Some(Outcome::Events(events, None))) => {
                let stopped = done_of(events).and_then(|done| done.finish_reason.as_deref());
                text_of(events) == text && stopped == Some(finish)
            }
            _ => false,
        }
    }
}

const REDIRECT: &str = "A redirect is not followed: the maintainer's decision.";

/// The failure of a redirect of `status`, after the one request.
const fn not_followed(status: u16, says: &'static str) -> Instead {
    fails(Some(status), false, says)
}

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
        not_followed(
            307,
            "the endpoint answered with a redirect (307), which is not followed",
        ),
    ),
    (
        "redirect_301",
        REDIRECT,
        not_followed(
            301,
            "the endpoint answered with a redirect (301), which is not followed",
        ),
    ),
    (
        "redirect_302_to_another_host",
        REDIRECT,
        not_followed(
            302,
            "the endpoint answered with a redirect (302), which is not followed",
        ),
    ),
    (
        "redirect_303",
        REDIRECT,
        not_followed(
            303,
            "the endpoint answered with a redirect (303), which is not followed",
        ),
    ),
    (
        "redirect_308",
        REDIRECT,
        not_followed(
            308,
            "the endpoint answered with a redirect (308), which is not followed",
        ),
    ),
    (
        "redirects_past_the_hop_limit",
        REDIRECT,
        not_followed(
            307,
            "the endpoint answered with a redirect (307), which is not followed",
        ),
    ),
    (
        "usage_completion_tokens_as_text",
        COUNT,
        counts(r#"{"input_tokens":12}"#),
    ),
    ("usage_counts_that_are_true", COUNT, counts("{}")),
    ("usage_cached_tokens_as_text", COUNT, counts(COUNTED)),
    ("usage_reasoning_tokens_as_text", COUNT, counts(COUNTED)),
    (
        "usage_cache_write_written_as_text_in_the_details",
        COUNT,
        counts(COUNTED),
    ),
    ("usage_counts_below_zero", COUNT, counts("{}")),
    (
        "stream_usage_completion_tokens_as_text",
        COUNT,
        counts(r#"{"input_tokens":12}"#),
    ),
    ("stream_usage_counts_that_are_true", COUNT, counts("{}")),
    ("stream_usage_cached_tokens_as_text", COUNT, counts(COUNTED)),
    (
        "stream_usage_reasoning_tokens_as_text",
        COUNT,
        counts(COUNTED),
    ),
    (
        "stream_last_event_without_its_blank_line",
        LAST_EVENT,
        streams("Hello.", "stop"),
    ),
    (
        "stream_last_event_without_its_line_end",
        LAST_EVENT,
        streams("Hello.", "stop"),
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
const ONLY_PYTHON: &[(&str, &str, Instead)] = &[
    (
        "empty_key",
        "The openai SDK builds no client without a key. The provider is built, as the \
         source's, and sends no Authorization.",
        Instead {
            requests: 1,
            does: Does::AsksWithoutAKey,
        },
    ),
    (
        "choices_that_are_no_list",
        "Python raises a raw AttributeError. The body is no completion.",
        fails(
            None,
            false,
            "The provider answered with a body it could not read: not a chat completion",
        ),
    ),
    (
        "usage_prompt_tokens_as_text",
        "Python raises a raw TypeError. The count stays unknown.",
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "usage_cached_tokens_as_a_word",
        "Python raises a raw TypeError. The count stays unknown.",
        counts(COUNTED),
    ),
    (
        "usage_completion_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "usage_counts_with_a_fraction",
        "Python raises a raw ValidationError. The counts stay unknown.",
        counts("{}"),
    ),
    (
        "usage_reasoning_tokens_as_a_word",
        "Python raises a raw ValidationError. The count stays unknown.",
        counts(COUNTED),
    ),
    (
        "list_models_status_401",
        SDK_S_OWN,
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
        fails(None, true, "the connection to the provider failed"),
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
        fails(
            Some(502),
            true,
            "the provider answered HTTP 502: Bad Gateway",
        ),
    ),
    (
        "status_400_with_a_plain_text",
        NO_ECHO,
        fails(
            Some(400),
            false,
            "the provider answered HTTP 400: Bad Request",
        ),
    ),
    (
        "context_overflow_by_its_words",
        "The failure says the context overflowed: its text does not repeat the body, which \
         says so outside its error's message, so the body is read for it. Python leaves the \
         question open, for a reader of its message, which holds the body.",
        Instead {
            requests: 1,
            does: Does::Fails(
                Some(400),
                false,
                Some(true),
                Says::Exactly("the provider answered HTTP 400: Bad Request"),
            ),
        },
    ),
    (
        "stream_usage_prompt_tokens_as_text",
        STREAM_COUNT,
        counts(r#"{"output_tokens":5}"#),
    ),
    (
        "stream_usage_completion_tokens_as_a_word",
        STREAM_COUNT,
        counts(r#"{"input_tokens":12}"#),
    ),
    (
        "stream_usage_counts_with_a_fraction",
        STREAM_COUNT,
        counts("{}"),
    ),
    (
        "stream_usage_cached_tokens_as_a_word",
        STREAM_COUNT,
        counts(COUNTED),
    ),
    (
        "stream_usage_reasoning_tokens_as_a_word",
        STREAM_COUNT,
        counts(COUNTED),
    ),
    (
        "stream_event_with_no_data",
        "An event that holds no data is no event, as the format of a stream of events \
         says. Python's client reads it as JSON and fails.",
        streams("Hello.", "stop"),
    ),
    (
        "stream_call_index_that_is_a_text",
        "A call's index is a whole number: another fails the stream. Python takes any \
         value as the key of the call.",
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
