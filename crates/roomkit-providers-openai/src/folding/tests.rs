//! The folding of a stream's chunks, ported from the unit tests of the
//! crate this provider was written from (`src/wire.rs::tests`, each named
//! on its test), which name Python's.

use std::collections::HashSet;

use roomkit_providers_llm::{ProviderError, StreamEvent};
use serde_json::{Value, json};

use super::Folding;
use crate::call_slots::RawCall;
use crate::failure::Failure;
use crate::failure::exit::test_exit;

fn folding() -> Folding<'static> {
    Folding::new("asked-model")
}

/// `failure` as it leaves a provider whose key is the placeholder.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the failure of a result, taken as `map_err` gives it"
)]
fn left(failure: Failure) -> ProviderError {
    test_exit().leave(&failure)
}

/// A chunk whose delta carries one `tool_calls` entry.
fn chunk(call: &Value) -> String {
    json!({"choices": [{"index": 0, "delta": {"tool_calls": [call]}}]}).to_string()
}

/// The calls of a slot-shape test: an id the provider minted is unique and
/// starts `call_`, and is written empty here, where the test compares the
/// calls; the server's ids stay.
fn normalize_minted_ids(mut calls: Vec<RawCall>, server_ids: &HashSet<&str>) -> Vec<RawCall> {
    assert!(calls.iter().all(|call| !call.id.is_empty()));
    let unique: HashSet<_> = calls.iter().map(|call| &call.id).collect();
    assert_eq!(unique.len(), calls.len());
    for call in &mut calls {
        if !server_ids.contains(call.id.as_str()) {
            assert!(call.id.starts_with("call_"), "{}", call.id);
            call.id.clear();
        }
    }
    calls
}

/// The calls a stream of one chunk per `fragments`, each a `tool_calls`
/// entry, folds into, or why its read failed.
fn streamed(fragments: &[Value]) -> Result<Vec<RawCall>, ProviderError> {
    let mut folding = folding();
    for fragment in fragments {
        let taken = folding.take(&chunk(fragment), &mut Vec::new());
        taken.map_err(left)?;
    }
    let ids = fragments.iter().filter_map(|f| f["id"].as_str()).collect();
    let (calls, _) = folding.finish().map_err(left)?;
    Ok(normalize_minted_ids(calls, &ids))
}

/// The calls `fragments`, each an index and a `tool_calls` entry's members,
/// fold into.
fn folded(fragments: &[(u64, Value)]) -> Vec<RawCall> {
    let entries = fragments.iter().map(|(index, fragment)| {
        let mut call = fragment.clone();
        call["index"] = (*index).into();
        call
    });
    streamed(&entries.collect::<Vec<_>>()).unwrap()
}

fn fragment(id: Option<&str>, name: Option<&str>, arguments: &str) -> Value {
    json!({"id": id, "function": {"name": name, "arguments": arguments}})
}

/// The text of each text delta of `events`.
fn texts(events: &[StreamEvent]) -> Vec<&str> {
    let texts = events.iter().filter_map(|event| match event {
        StreamEvent::TextDelta(delta) => Some(delta.text.as_str()),
        StreamEvent::ToolCall(_)
        | StreamEvent::ToolCallDelta(_)
        | StreamEvent::Done(_)
        | StreamEvent::ThinkingDelta(_)
        | StreamEvent::Usage(_) => None,
    });
    texts.collect()
}

/// A failure of a chunk that does not read: final, with no status.
fn is_unread(read: &Result<Vec<RawCall>, ProviderError>) -> bool {
    read.as_ref()
        .is_err_and(|error| !error.retryable && error.status_code.is_none())
}

// Source: src/wire.rs::tests::whole_calls_on_one_index_stay_two_calls
// Python: tests/test_providers/test_tool_call_handoff.py::TestStreamedSlots::test_whole_calls_on_one_index_stay_two_calls
#[test]
fn whole_calls_on_one_index_stay_two_calls() {
    let calls = folded(&[
        (0, fragment(Some("a"), Some("now"), r#"{"tz": "A"}"#)),
        (0, fragment(Some("b"), Some("later"), r#"{"tz": "B"}"#)),
    ]);
    assert_eq!(
        calls,
        [
            RawCall::new("a", "now", r#"{"tz": "A"}"#),
            RawCall::new("b", "later", r#"{"tz": "B"}"#),
        ]
    );
}

// Source: src/wire.rs::tests::whole_calls_without_ids_on_one_index_stay_two_calls
// Python: tests/test_providers/test_tool_call_handoff.py::TestStreamedSlots::test_whole_calls_without_ids_on_one_index_stay_two_calls
#[test]
fn whole_calls_without_ids_on_one_index_stay_two_calls() {
    let calls = folded(&[
        (0, fragment(None, Some("roll_die"), r#"{"sides": 6}"#)),
        (0, fragment(None, Some("roll_die"), r#"{"sides": 6}"#)),
    ]);
    assert_eq!(
        calls,
        [
            RawCall::new("", "roll_die", r#"{"sides": 6}"#),
            RawCall::new("", "roll_die", r#"{"sides": 6}"#),
        ]
    );
}

// Source: src/wire.rs::tests::fragments_of_one_call_fold_together
// Python: tests/test_providers/test_tool_call_handoff.py::TestStreamedSlots::test_fragments_of_one_call_fold_together
#[test]
fn fragments_of_one_call_fold_together() {
    let calls = folded(&[
        (0, fragment(Some("a"), Some("now"), r#"{"tz": "#)),
        (0, fragment(None, None, r#""A"}"#)),
    ]);
    assert_eq!(calls, [RawCall::new("a", "now", r#"{"tz": "A"}"#)]);
}

// Source: src/wire.rs::tests::what_starts_another_call_on_an_occupied_index
// Python: providers/ai/openai_dialect.py ToolCallSlots._starts_another_call:
// a server may repeat a call's name on every fragment; after whole arguments
// a new JSON object under no name opens a second call to the same tool.
#[test]
fn what_starts_another_call_on_an_occupied_index() {
    let repeated = folded(&[
        (0, fragment(None, Some("now"), r#"{"tz": "#)),
        (0, fragment(None, Some("now"), r#""A"}"#)),
    ]);
    assert_eq!(repeated, [RawCall::new("", "now", r#"{"tz": "A"}"#)]);
    let nameless = folded(&[
        (0, fragment(Some("a"), Some("now"), r#"{"tz": "A"}"#)),
        (0, fragment(None, None, r#"{"tz": "B"}"#)),
    ]);
    assert_eq!(
        nameless,
        [
            RawCall::new("a", "now", r#"{"tz": "A"}"#),
            RawCall::new("", "now", r#"{"tz": "B"}"#),
        ]
    );
    let other_name_mid_arguments = folded(&[
        (0, fragment(Some("a"), Some("now"), r#"{"tz": "#)),
        (0, fragment(None, Some("later"), r#""A"}"#)),
    ]);
    assert_eq!(
        other_name_mid_arguments,
        [RawCall::new("a", "later", r#"{"tz": "A"}"#)]
    );
}

// Source: src/wire.rs::tests::an_entry_with_no_function_is_no_call
// Python: providers/ai/openai_dialect.py ToolCallSlots.calls: an entry with
// no function is no call (a custom tool's), its id kept for a function a
// later fragment brings; one whose function has no name and no arguments is
// a call, for the host to refuse.
#[test]
fn an_entry_with_no_function_is_no_call() {
    let no_function = json!({"id": "a"});
    assert_eq!(folded(&[(0, no_function.clone())]), []);
    let empty = json!({"id": "a", "function": {}});
    assert_eq!(folded(&[(0, empty)]), [RawCall::new("a", "", "")]);
    assert_eq!(
        folded(&[(0, no_function), (0, fragment(None, Some("now"), "{}"))]),
        [RawCall::new("a", "now", "{}")]
    );
}

// Source: src/wire.rs::tests::what_a_fragment_on_an_occupied_index_folds_into
// Python: providers/ai/openai_dialect.py ToolCallSlots, its answers read from
// Python: each fragment an id, a name and its arguments, and the calls they
// fold into.
#[test]
fn what_a_fragment_on_an_occupied_index_folds_into() {
    type Part = (Option<&'static str>, Option<&'static str>, &'static str);
    let cases: [(&[Part], Vec<RawCall>); 9] = [
        (
            &[
                (Some("a"), Some("now"), r#"{"o": "#),
                (None, None, r#"{"k": 1}}"#),
            ],
            vec![RawCall::new("a", "now", r#"{"o": {"k": 1}}"#)],
        ),
        (
            &[(Some("a"), Some("now"), "{}"), (None, None, "\n")],
            vec![RawCall::new("a", "now", "{}\n")],
        ),
        (
            &[(Some("a"), Some("now"), "{}"), (None, Some("now"), " ")],
            vec![RawCall::new("a", "now", "{} ")],
        ),
        (
            &[(Some("a"), Some("now"), ""), (None, Some("now"), "{}")],
            vec![RawCall::new("a", "now", "{}")],
        ),
        (
            &[(Some("a"), Some("now"), ""), (None, Some("later"), "{}")],
            vec![
                RawCall::new("a", "now", ""),
                RawCall::new("", "later", "{}"),
            ],
        ),
        (
            &[(Some("a"), Some("now"), "{}"), (Some("b"), None, "{}")],
            vec![RawCall::new("a", "now", "{}"), RawCall::new("b", "", "{}")],
        ),
        (
            &[(Some("a"), Some("now"), "{}"), (Some("a"), None, "{}")],
            vec![
                RawCall::new("a", "now", "{}"),
                RawCall::new("", "now", "{}"),
            ],
        ),
        // Named before its server gave an id, with no arguments yet.
        (
            &[(None, Some("now"), ""), (Some("a"), None, "{}")],
            vec![RawCall::new("", "now", "{}")],
        ),
        // The call was named before its server gave an id: no id starts
        // another call, none is taken.
        (
            &[
                (None, Some("now"), r#"{"tz": "#),
                (Some("a"), None, r#""A""#),
                (Some("b"), None, "}"),
            ],
            vec![RawCall::new("", "now", r#"{"tz": "A"}"#)],
        ),
    ];
    for (parts, read) in cases {
        let fragments: Vec<(u64, Value)> = parts
            .iter()
            .map(|(id, name, arguments)| (0, fragment(*id, *name, arguments)))
            .collect();
        assert_eq!(folded(&fragments), read, "{parts:?}");
    }
}

// Source: src/wire.rs::tests::a_streamed_id_that_reads_as_false_is_no_id
// Python: providers/ai/openai_dialect.py ToolCallSlots: an id Python reads as
// false is no id, and the read goes on.
#[test]
fn a_streamed_id_that_reads_as_false_is_no_id() {
    for id in ["0", "0.0", "false", "[]", "{}"] {
        let entry = json!({
            "index": 0, "id": serde_json::from_str::<Value>(id).unwrap(),
            "function": {"name": "now", "arguments": "{}"},
        });
        let read = streamed(&[entry]).unwrap();
        assert_eq!(read, [RawCall::new("", "now", "{}")], "{id}");
    }
}

// Source: src/wire.rs::tests::an_id_that_is_no_text_on_a_new_call_fails_the_read
// Python: tests/test_providers/test_chat_response_reading.py::test_an_id_that_is_no_text_on_a_new_call_fails_the_read
#[test]
fn an_id_that_is_no_text_on_a_new_call_fails_the_read() {
    let named = |id: Value| {
        json!({"index": 0, "id": id, "type": "function",
            "function": {"name": "lookup", "arguments": "{}"}})
    };
    let entries = [
        named(json!(7)),
        named(json!(true)),
        named(json!([1])),
        json!({"index": 0, "id": {"a": 1}, "type": "custom"}),
    ];
    for entry in entries {
        let read = streamed(std::slice::from_ref(&entry));
        assert!(is_unread(&read), "{entry}: {read:?}");
    }
}

/// Python's `NAMED`: a whole call to `lookup` on index 1, under no id.
fn named() -> Value {
    json!({"index": 1, "type": "function",
        "function": {"name": "lookup", "arguments": "{}"}})
}

// Source: src/wire.rs::tests::an_id_that_is_no_text_another_entry_holds_is_not_taken_again
// Python: tests/test_providers/test_chat_response_reading.py::test_an_id_that_is_no_text_another_entry_holds_is_not_taken_again
#[test]
fn an_id_that_is_no_text_another_entry_holds_is_not_taken_again() {
    let pairs = [
        (json!(1), json!(true)),
        (json!(true), json!(1)),
        (json!(7), json!(7.0)),
        (json!(7), json!(7)),
    ];
    for (held, again) in pairs {
        let entry = json!({"index": 0, "id": held, "type": "custom"});
        let mut call = named();
        call["id"] = again.clone();
        let read = streamed(&[entry, call]).unwrap();
        assert_eq!(
            read,
            [RawCall::new("", "lookup", "{}")],
            "{held} then {again}"
        );
    }
}

// Source: src/wire.rs::tests::a_call_named_under_an_id_that_is_no_text_fails_where_it_is_named
// Python: tests/test_providers/test_chat_response_reading.py::test_a_call_named_under_an_id_that_is_no_text_fails_where_it_is_named
#[test]
fn a_call_named_under_an_id_that_is_no_text_fails_where_it_is_named() {
    let pairs = [
        (json!(7), Value::Null),
        (json!(7), json!(7)),
        (json!(7), json!(7.0)),
        (json!(1), json!(true)),
        (json!(true), json!(1)),
    ];
    for (held, again) in pairs {
        let mut folding = folding();
        let entry = json!({"index": 0, "id": held, "type": "custom"});
        let mut call = named();
        call["index"] = 0.into();
        call["id"] = again.clone();
        let taken = folding.take(&chunk(&entry), &mut Vec::new());
        assert!(taken.is_ok(), "{held}: {taken:?}");
        let read = folding.take(&chunk(&call), &mut Vec::new()).map_err(left);
        // Source: a malformed answer, which is final and names no status.
        let failed = read
            .as_ref()
            .is_err_and(|error| !error.retryable && error.status_code.is_none());
        assert!(failed, "{held} then {again}: {read:?}");
    }
}

/// A whole call to `lookup` under the server's id `c1`, Python's `WHOLE`.
fn whole_call() -> Value {
    json!({"index": 0, "id": "c1", "type": "function",
        "function": {"name": "lookup", "arguments": "{}"}})
}

// Source: src/wire.rs::tests::a_nameless_call_under_an_id_that_is_no_text_takes_no_name
// Python: tests/test_providers/test_chat_response_reading.py::test_a_nameless_call_under_an_id_that_is_no_text_takes_no_name
#[test]
fn a_nameless_call_under_an_id_that_is_no_text_takes_no_name() {
    for id in [json!(0), json!([]), json!({}), json!(false)] {
        let second = json!({"index": 0, "id": id, "function": {"arguments": "{\"q\": 1}"}});
        let read = streamed(&[whole_call(), second]).unwrap();
        let expected = [
            RawCall::new("c1", "lookup", "{}"),
            RawCall::new("", "", r#"{"q": 1}"#),
        ];
        assert_eq!(read, expected, "{id}");
    }
}

// Source: src/wire.rs::tests::a_nameless_call_under_no_id_or_the_held_one_s_takes_its_name
// Under no id, or the held call's own, a nameless fragment opening another
// JSON object is a second call to the held call's tool.
#[test]
fn a_nameless_call_under_no_id_or_the_held_one_s_takes_its_name() {
    for id in [Value::Null, json!(""), json!("c1")] {
        let second = json!({"index": 0, "id": id, "function": {"arguments": "{\"q\": 1}"}});
        let read = streamed(&[whole_call(), second]).unwrap();
        let expected = [
            RawCall::new("c1", "lookup", "{}"),
            RawCall::new("", "lookup", r#"{"q": 1}"#),
        ];
        assert_eq!(read, expected, "{id}");
    }
}

// Source: src/wire.rs::tests::an_id_that_is_no_text_on_a_call_already_named_is_ignored
// Python: tests/test_providers/test_chat_response_reading.py::test_an_id_that_is_no_text_on_a_call_already_named_is_ignored
#[test]
fn an_id_that_is_no_text_on_a_call_already_named_is_ignored() {
    for id in [json!(7), json!(true), json!([1]), json!({"a": 1})] {
        let first = json!({"index": 0, "type": "function",
            "function": {"name": "lookup", "arguments": "{\"q\""}});
        let second = json!({"index": 0, "id": id, "function": {"arguments": ": 1}"}});
        let read = streamed(&[first, second]).unwrap();
        assert_eq!(read, [RawCall::new("", "lookup", r#"{"q": 1}"#)], "{id}");
    }
}

// Source: src/wire.rs::tests::an_id_that_is_no_text_on_an_entry_without_a_function_is_no_call
// Python: tests/test_providers/test_chat_response_reading.py::test_an_id_that_is_no_text_on_an_entry_without_a_function_is_no_call
#[test]
fn an_id_that_is_no_text_on_an_entry_without_a_function_is_no_call() {
    for id in [json!(7), json!(true)] {
        let entry = json!({"index": 0, "id": id, "type": "custom"});
        let named = json!({"index": 1, "id": "c2", "type": "function",
            "function": {"name": "lookup", "arguments": "{}"}});
        let read = streamed(&[entry, named]).unwrap();
        assert_eq!(read, [RawCall::new("c2", "lookup", "{}")], "{id}");
    }
}

// Source: src/wire.rs::tests::a_server_id_another_call_holds_is_not_taken
// Python: providers/ai/openai_dialect.py ToolCallSlots._adopt_server_id: an
// id another call of the response holds stays with that call.
#[test]
fn a_server_id_another_call_holds_is_not_taken() {
    let calls = folded(&[
        (0, fragment(Some("a"), Some("now"), "{}")),
        (1, fragment(Some("a"), Some("later"), "{}")),
    ]);
    assert_eq!(
        calls,
        [
            RawCall::new("a", "now", "{}"),
            RawCall::new("", "later", "{}"),
        ]
    );
}

// Source: src/wire.rs::tests::a_streamed_completion_s_call_fragments_fold_by_index
// Python: providers/ai/openai_dialect.py ToolCallSlots: each call in the
// order it came, whatever index it names; its id and name kept from the
// first fragment that gives them, its arguments' text joined; the usage
// comes in the last chunk.
#[test]
fn a_streamed_completion_s_call_fragments_fold_by_index() {
    let mut folding = folding();
    let chunks = [
        r#"{"choices": [{"index": 0, "delta": {"role": "assistant", "content": "Let me "}}]}"#,
        r#"{"choices": [{"index": 0, "delta": {"content": "check."}}]}"#,
        r#"{"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 1, "id": "call_b",
            "function": {"name": "notify", "arguments": ""}}]}}]}"#,
        r#"{"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_a",
            "function": {"name": "lookup", "arguments": "{\"or"}}]}}]}"#,
        r#"{"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0,
            "function": {"arguments": "der\": \"42\"}"}}]}}]}"#,
        r#"{"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}"#,
        r#"{"choices": [], "usage": {"prompt_tokens": 9, "completion_tokens": 4}}"#,
    ];
    let mut ready = Vec::new();
    for chunk in chunks {
        folding.take(chunk, &mut ready).unwrap();
    }
    assert_eq!(texts(&ready), ["Let me ", "check."]);
    let reasoned = |event: &StreamEvent| matches!(event, StreamEvent::ThinkingDelta(_));
    assert!(!ready.iter().any(reasoned), "{ready:?}");
    let (calls, done) = folding.finish().unwrap();
    assert_eq!(
        calls,
        [
            RawCall::new("call_b", "notify", ""),
            RawCall::new("call_a", "lookup", r#"{"order": "42"}"#),
        ]
    );
    assert_eq!(
        (done.finish_reason.as_deref(), Value::Object(done.usage)),
        (
            Some("tool_calls"),
            json!({"input_tokens": 9, "output_tokens": 4})
        )
    );
    // The end names the model asked where no chunk named one.
    assert_eq!(done.metadata.get("model"), Some(&json!("asked-model")));
}

// Source: src/wire.rs::tests::a_chunk_reporting_an_error_fails_without_the_key
#[test]
fn a_chunk_reporting_an_error_fails_without_the_key() {
    let mut folding = folding();
    let failed = folding.take(
        r#"{"error": {"message": "overloaded, key placeholder-not-a-key"}}"#,
        &mut Vec::new(),
    );
    let error = left(failed.unwrap_err());
    assert_eq!(
        (error.message.as_str(), error.status_code, error.retryable),
        ("overloaded, key [***]", None, true)
    );
}

// What Python reads of a chunk beside its first choice's delta: the first
// choice of the list whatever its index, a finish reason that says
// something, a usage that is an object.
#[test]
fn a_chunk_is_read_by_its_first_choice() {
    let mut folding = folding();
    let mut ready = Vec::new();
    let chunks = [
        json!({"model": "answering", "choices": [
            {"index": 1, "delta": {"content": "first"}, "finish_reason": "stop"},
            {"index": 0, "delta": {"content": "second"}, "finish_reason": "length"},
        ]}),
        json!({"model": "", "choices": [{"index": 0, "delta": {}, "finish_reason": ""}]}),
        json!({"choices": null, "usage": {"prompt_tokens": 3}}),
        json!({"usage": null}),
        json!({"choices": [{"index": 0}]}),
        // What reads as nothing: a delta that is no object, a finish reason
        // and arguments Python reads as false.
        json!({"choices": [{"delta": "Hel", "finish_reason": 0}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": 0}}]}}]}),
    ];
    for chunk in chunks {
        folding.take(&chunk.to_string(), &mut ready).unwrap();
    }
    assert_eq!(texts(&ready), ["first"]);
    let (_, done) = folding.finish().unwrap();
    assert_eq!(done.finish_reason.as_deref(), Some("stop"));
    assert_eq!(Value::Object(done.usage), json!({"input_tokens": 3}));
    assert_eq!(done.metadata.get("model"), Some(&json!("answering")));
}

// What fails a stream in Python fails it here, final: a chunk that is no
// object, choices or calls that are no list, a choice that is no object, a
// text that is no text.
#[test]
fn a_chunk_that_does_not_read_fails_the_stream() {
    let chunks = [
        json!([1, 2]),
        json!("text"),
        json!({"choices": "abc"}),
        json!({"choices": [null]}),
        json!({"choices": [{"delta": {"tool_calls": {"a": 1}}}]}),
        json!({"choices": [{"delta": {"tool_calls": ["a"]}}]}),
        json!({"choices": [{"delta": {"content": 7}}]}),
        json!({"choices": [{"delta": {"content": ["a"]}}]}),
        json!({"choices": [{"finish_reason": 3}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": 7}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"function": {"arguments": [1]}}]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [{"index": "0"}]}}]}),
    ];
    for chunk in chunks {
        let failed = folding().take(&chunk.to_string(), &mut Vec::new());
        let error = left(failed.expect_err(&chunk.to_string()));
        assert_eq!(
            (error.retryable, error.status_code),
            (false, None),
            "{chunk}"
        );
    }
    let no_json = left(folding().take("{not json", &mut Vec::new()).unwrap_err());
    assert_eq!((no_json.retryable, no_json.status_code), (false, None));
}

// A name that is empty is no name, as Python reads it: the fragment goes
// on with the call it follows, where a name would start another.
#[test]
fn an_empty_name_is_no_name() {
    let whole = json!({"index": 0, "id": "a", "function": {"name": "now", "arguments": "{}"}});
    let unnamed = json!({"index": 0, "function": {"name": "", "arguments": " "}});
    let read = streamed(&[whole.clone(), unnamed]).unwrap();
    assert_eq!(read, [RawCall::new("a", "now", "{} ")]);
    let renamed = json!({"index": 0, "function": {"name": "later", "arguments": " "}});
    let read = streamed(&[whole, renamed]).unwrap();
    assert_eq!(
        read,
        [
            RawCall::new("a", "now", "{}"),
            RawCall::new("", "later", " ")
        ]
    );
}

// The refusal a stream carries is kept within what an answer read whole
// may weigh.
#[test]
fn a_refusal_past_its_bound_fails_the_stream() {
    let piece = json!({"choices": [{"delta": {"refusal": "x".repeat(512 * 1024)}}]}).to_string();
    let mut folding = folding();
    for _ in 0..8 {
        folding.take(&piece, &mut Vec::new()).unwrap();
    }
    let error = left(folding.take(&piece, &mut Vec::new()).unwrap_err());
    assert_eq!(
        (error.message.as_str(), error.retryable, error.status_code),
        (
            "The provider answered with a body it could not read: the refusal is longer than \
             4194304 bytes",
            false,
            None
        )
    );
}
