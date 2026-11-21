//! The cases the provider answers otherwise than Python: each with the
//! reason, how its replay then differs from Python's, and exactly what the
//! provider does instead. The replay asserts both: it differs in what the
//! entry states and in nothing else, and the provider sends as many
//! requests and gives what the entry states.

use std::collections::BTreeMap;

use roomkit_providers_llm::{ProviderError, StreamDone, StreamEvent};

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
    /// The call is answered, whole or as a stream that ends without a
    /// failure: it says this text, stops for this reason, and counts this
    /// usage.
    Answers(&'static str, &'static str, &'static str),
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

/// A named case as its entry states it: how its replay differs from
/// Python's, each difference by the words it starts with, and what the
/// provider does instead.
#[derive(Clone, Copy, Debug)]
pub(super) struct Stated {
    differs: &'static [&'static str],
    instead: Instead,
}

impl Stated {
    /// Whether the replay is what the entry states: `differing`, the
    /// differences found from Python, are the ones stated, each of them
    /// and no other, and `replayed` did what is stated.
    ///
    /// # Errors
    ///
    /// What is not as stated.
    pub(super) fn holds(&self, differing: &[String], replayed: &Replayed) -> Result<(), String> {
        let as_stated = differing.len() == self.differs.len()
            && differing
                .iter()
                .zip(self.differs)
                .all(|(found, stated)| found.starts_with(stated));
        if !as_stated {
            return Err(format!(
                "it differs from Python otherwise than its entry states, {:?}: {differing:?}",
                self.differs
            ));
        }
        if !self.instead.is_done(replayed) {
            let (unbuilt, outcome) = (&replayed.unbuilt, &replayed.outcome);
            return Err(format!("not {:?}: {unbuilt:?} {outcome:?}", self.instead));
        }
        Ok(())
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
            (Does::Answers(text, finish, usage), None, Some(Outcome::Response(Ok(response)))) => {
                response.content == text
                    && response.finish_reason.as_deref() == Some(finish)
                    && serde_json::to_string(&response.usage).unwrap() == usage
            }
            (Does::Answers(text, finish, usage), None, Some(Outcome::Events(events, None))) => {
                let done = done_of(events);
                let stopped = done.and_then(|done| done.finish_reason.as_deref());
                let counted = done.map(|done| serde_json::to_string(&done.usage).unwrap());
                text_of(events) == text
                    && stopped == Some(finish)
                    && counted.as_deref() == Some(usage)
            }
            _ => false,
        }
    }
}

/// A named case: its name in the fixture, why the provider answers it
/// otherwise than Python, how its replay then differs from Python's, each
/// difference by the words it starts with, and what the provider does
/// instead.
type Entry = (&'static str, &'static str, &'static [&'static str], Instead);

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
        "The failure says the context overflowed: its text does not repeat the body, which \
         says so outside its error's message, so the body is read for it. Python leaves the \
         question open, for a reader of its message, which holds the body.",
        &["the failure's context_overflow differs: "],
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

/// The cases the provider answers otherwise than Python, by name: what
/// the entry of each states.
pub(super) fn named() -> BTreeMap<&'static str, Stated> {
    let lists = [QUESTIONED_OTHERWISE, ONLY_PYTHON, BY_ITS_OWN_RULE];
    let named = lists.into_iter().flatten();
    let stated = |(name, _, differs, instead): &Entry| {
        let (differs, instead) = (*differs, *instead);
        (*name, Stated { differs, instead })
    };
    named.map(stated).collect()
}

/// What a failure says, as the data file writes it.
fn says_as_data(says: Says) -> serde_json::Value {
    match says {
        Says::Exactly(words) => serde_json::json!({"exactly": words}),
        Says::Holding(words) => serde_json::json!({"holding": words}),
    }
}

/// What the provider does of a named case, as the data file writes it.
fn does_as_data(does: Does) -> serde_json::Value {
    match does {
        Does::Unbuilt(says) => serde_json::json!({"kind": "unbuilt", "says": says}),
        Does::Fails(status_code, retryable, context_overflow, says) => serde_json::json!({
            "kind": "fails",
            "status_code": status_code,
            "retryable": retryable,
            "context_overflow": context_overflow,
            "says": says_as_data(says),
        }),
        Does::Answers(text, finish_reason, usage) => serde_json::json!({
            "kind": "answers",
            "text": text,
            "finish_reason": finish_reason,
            "usage": serde_json::from_str::<serde_json::Value>(usage).unwrap(),
        }),
        Does::AsksWithoutAKey => serde_json::json!({"kind": "asks_without_a_key"}),
    }
}

/// The named cases as a data file, for a replay outside this crate: each
/// with the list it stands in, its reason, the differences from Python its
/// replay shows and what the provider does instead.
fn named_as_data() -> String {
    let lists = [
        ("questioned_otherwise", QUESTIONED_OTHERWISE),
        ("only_python", ONLY_PYTHON),
        ("by_its_own_rule", BY_ITS_OWN_RULE),
    ];
    let mut cases = Vec::new();
    for (list, entries) in lists {
        for (name, reason, differs, instead) in entries {
            cases.push(serde_json::json!({
                "name": name,
                "list": list,
                "reason": reason,
                "differs": differs,
                "requests": instead.requests,
                "does": does_as_data(instead.does),
            }));
        }
    }
    let notes = "The cases of python_chat_cases.json the provider answers otherwise than \
                 Python, as src/replay/named.rs states them, for a replay outside the crate. \
                 'differs' lists the differences from Python a replay finds, each by the words \
                 it starts with; 'requests' is how many requests the provider sends; 'does' \
                 is what the call gives. A test of the crate keeps this file equal to the \
                 source: rewrite it with make named-cases.";
    let file = serde_json::json!({"notes": notes, "cases": cases});
    serde_json::to_string_pretty(&file).unwrap() + "\n"
}

/// The data file of the named cases is what this module states. Run with
/// `NAMED_CASES_OUT` naming a file, it writes the file instead.
#[test]
fn the_named_cases_file_is_what_this_module_states() {
    let stated = named_as_data();
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
    let fixture = fixture();
    let case = |name: &str| {
        let found = fixture.cases.iter().find(|case| case.name == name);
        found.unwrap_or_else(|| panic!("no case {name}"))
    };
    let raw = |name: &str| {
        let compare = case(name).compare.as_ref();
        compare.is_some_and(|compare| compare.exactly.is_empty())
    };
    for (name, reason, differs, _) in QUESTIONED_OTHERWISE {
        assert!(case(name).questioned.is_some(), "{name}");
        assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
    }
    for (name, reason, differs, _) in ONLY_PYTHON {
        assert!(raw(name), "{name}");
        assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
    }
    for (name, reason, differs, _) in BY_ITS_OWN_RULE {
        assert!(case(name).questioned.is_none() && !raw(name), "{name}");
        assert!(reason.ends_with('.') && !differs.is_empty(), "{name}");
    }
    let listed = QUESTIONED_OTHERWISE.len() + ONLY_PYTHON.len() + BY_ITS_OWN_RULE.len();
    assert_eq!(named().len(), listed);
}
