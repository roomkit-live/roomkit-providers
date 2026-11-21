//! What the folding reads of a stream's events, one event after the other,
//! with no request: its blocks, its calls and its bounds.

use roomkit_providers_llm::{ProviderError, ThinkingBlocks};
use serde_json::json;

use super::calls::{ARGUMENTS_LIMIT, CALLS_LIMIT, NAME_LIMIT};
use super::*;
use crate::failure::test_exit;

/// Feeds `events` to a new folding: what they gave, and the failure that
/// stopped them, with the folding as they left it.
fn fold(events: &[Value]) -> (Vec<StreamEvent>, Option<ProviderError>, Folding<'static>) {
    let mut folding = Folding::new("asked");
    let mut ready = Vec::new();
    for event in events {
        if let Err(failure) = folding.take(&event.to_string(), &mut ready) {
            return (ready, Some(test_exit().leave(&failure)), folding);
        }
    }
    (ready, None, folding)
}

/// As [`fold`], for events that fold: what they gave, their end included.
fn folded(events: &[Value]) -> Vec<StreamEvent> {
    let (mut ready, failure, folding) = fold(events);
    assert!(failure.is_none(), "{failure:?}");
    assert!(folding.is_complete(), "the message is complete");
    folding.finish(&mut ready);
    ready
}

fn start() -> Value {
    json!({"type": "message_start", "message": {}})
}

fn opened(index: u32, block: &Value) -> Value {
    json!({"type": "content_block_start", "index": index, "content_block": block})
}

fn delta(index: u32, delta: &Value) -> Value {
    json!({"type": "content_block_delta", "index": index, "delta": delta})
}

fn closed(index: u32) -> Value {
    json!({"type": "content_block_stop", "index": index})
}

fn stopped(reason: &str) -> [Value; 2] {
    [
        json!({"type": "message_delta", "delta": {"stop_reason": reason}, "usage": {"output_tokens": 10}}),
        json!({"type": "message_stop"}),
    ]
}

fn call(id: &Value, name: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": name, "input": {}})
}

fn fragment(text: &str) -> Value {
    json!({"type": "input_json_delta", "partial_json": text})
}

// Source: src/tests.rs::thinking_deltas_and_signatures_keep_their_block_identity
// Python: tests/test_providers/test_anthropic.py::test_thinking_signature_captured_from_stream
// and tests/test_providers/test_thinking_blocks.py::test_each_block_keeps_its_text_and_signature
#[test]
fn thinking_deltas_and_signatures_keep_their_block_identity() {
    let (events, failure, _) = fold(&[
        start(),
        opened(0, &json!({"type": "thinking"})),
        delta(0, &json!({"type": "thinking_delta", "thinking": "first"})),
        delta(0, &json!({"type": "signature_delta", "signature": "S0"})),
        opened(2, &json!({"type": "redacted_thinking", "data": "opaque"})),
    ]);
    assert!(failure.is_none(), "{failure:?}");
    let mut blocks = ThinkingBlocks::new();
    for event in events {
        if let StreamEvent::ThinkingDelta(delta) = event {
            blocks.add(&delta);
        }
    }
    let parts = blocks.parts();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].thinking, "first");
    assert_eq!(parts[0].signature.as_deref(), Some("S0"));
    assert_eq!(parts[1].redacted.as_deref(), Some("opaque"));
}

// Source: src/tests.rs::a_message_start_tells_its_input
// Python: tests/test_providers/test_anthropic.py::TestAnthropicUsageCounters::test_a_message_start_tells_its_input
#[test]
fn a_message_start_tells_its_input() {
    let cached = json!({
        "input_tokens": 11,
        "output_tokens": 7,
        "cache_creation_input_tokens": 5,
        "cache_read_input_tokens": 3,
    });
    let cached_input = json!({
        "input_tokens": 11,
        "cache_creation_input_tokens": 5,
        "cache_read_input_tokens": 3,
    });
    for (usage, told) in [
        (
            json!({"input_tokens": 11, "output_tokens": 7}),
            json!({"input_tokens": 11}),
        ),
        (cached, cached_input),
    ] {
        let started = json!({"type": "message_start", "message": {"usage": usage}});
        let (events, failure, _) = fold(&[started]);
        assert!(failure.is_none(), "{failure:?}");
        let told = told.as_object().cloned().unwrap_or_default();
        assert_eq!(events, [StreamEvent::Usage(StreamUsage { usage: told })]);
    }
    // A start that tells nothing of its input gives no event.
    let (events, failure, _) = fold(&[start()]);
    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(events, []);
}

// Source: src/tests.rs::fragmented_calls_are_not_executable_when_the_output_cap_cuts_them
// Python: src/roomkit/providers/anthropic/tool_blocks.py::ToolUseBlocks.remaining
#[test]
fn fragmented_calls_are_not_executable_when_the_output_cap_cuts_them() {
    let usage = json!({"input_tokens": 3, "output_tokens": 1});
    let mut events = vec![
        json!({"type": "message_start", "message": {"usage": usage}}),
        opened(0, &call(&json!("c"), "lookup")),
        delta(0, &fragment("{\"x\":")),
    ];
    events.extend(stopped("max_tokens"));
    let events = folded(&events);
    let [
        StreamEvent::Usage(_),
        StreamEvent::ToolCallDelta(_),
        StreamEvent::ToolCallDelta(_),
        StreamEvent::ToolCall(call),
        StreamEvent::Done(done),
    ] = events.as_slice()
    else {
        panic!("unexpected events: {events:?}");
    };
    assert!(call.partial && !call.garbled);
    assert_eq!(call.arguments["raw"], "{\"x\":");
    assert_eq!(done.usage["output_tokens"], 10);
    assert_eq!(done.usage["input_tokens"], 3);
    assert_eq!(done.metadata["model"], "asked");
}

// Source: src/tests/wire.rs::malformed_known_events_cannot_finish_successfully
// Python: the anthropic SDK fails a stream on an event it cannot fold;
// here each of these fails it, the ones the SDK folds without a word too.
#[test]
fn malformed_known_events_cannot_finish_successfully() {
    let text = || opened(0, &json!({"type": "text"}));
    let said = |index: u32, text: &str| delta(index, &json!({"type": "text_delta", "text": text}));
    let thought = delta(
        0,
        &json!({"type": "thinking_delta", "thinking": "wrong block"}),
    );
    let cases = [
        (
            vec![json!({"type": "message_delta", "delta": {}})],
            "before its start",
        ),
        (vec![json!({"type": "message_stop"})], "before its start"),
        (
            vec![start(), said(42, "orphan")],
            "names no open content block",
        ),
        (vec![start(), text(), text()], "opens twice"),
        (
            vec![start(), text(), thought],
            "does not match its content block",
        ),
        (
            vec![start(), text(), closed(0), said(0, "closed")],
            "names no open content block",
        ),
        (vec![start(), closed(42)], "closes that is not open"),
        (vec![start(), start()], "starts twice"),
        (vec![start(), json!([])], "is no object"),
        (vec![start(), json!({"index": 0})], "names no type"),
        (
            vec![start(), opened(0, &json!({"type": "tool_use"}))],
            "names no tool",
        ),
        (
            vec![json!({"type": "content_block_start", "index": -1, "content_block": {}})],
            "before its start",
        ),
        (
            vec![
                start(),
                json!({"type": "content_block_start", "index": "0", "content_block": {}}),
            ],
            "names no index",
        ),
    ];
    for (events, why) in cases {
        let (ready, failure, folding) = fold(&events);
        let error = failure.unwrap_or_else(|| panic!("{events:?} fold"));
        assert!(error.message.contains(why), "{}: {why}", error.message);
        assert_eq!((error.retryable, error.status_code), (false, None));
        assert!(!folding.is_complete(), "{events:?}");
        assert!(
            !ready
                .iter()
                .any(|event| matches!(event, StreamEvent::Done(_)))
        );
    }
}

// Source: src/tests/wire.rs::tool_fragments_keep_the_final_call_identity
// Python: tests/test_providers/test_anthropic.py::TestAnthropicAIProvider::test_structured_stream_surfaces_a_tool_call_as_it_is_composed
#[test]
fn tool_fragments_keep_the_final_call_identity() {
    let mut events = vec![
        start(),
        opened(0, &call(&json!("call-a"), "lookup")),
        delta(0, &fragment("{\"n\":1}")),
        closed(0),
    ];
    events.extend(stopped("tool_use"));
    let events = folded(&events);
    let [
        StreamEvent::ToolCallDelta(open),
        StreamEvent::ToolCallDelta(fragment),
        StreamEvent::ToolCall(call),
        StreamEvent::Done(_),
    ] = events.as_slice()
    else {
        panic!("unexpected events: {events:?}");
    };
    assert_eq!(
        (open.id.as_str(), fragment.id.as_str()),
        ("call-a", "call-a")
    );
    assert_eq!(call.id, "call-a");
    assert_eq!(open.name, call.name);
    assert_eq!(open.arguments_delta, "");
    assert_eq!(fragment.arguments_delta, "{\"n\":1}");
    assert_eq!(call.arguments["n"], 1);
    assert!(!call.partial && !call.garbled);
}

// Each call is handed once, under an id of its own: the server's as Python
// reads what it wrote, unless an earlier call of the message took it.
#[test]
fn each_call_is_handed_once_under_an_id_of_its_own() {
    let ids = [
        json!("toolu_1"),
        json!("toolu_1"),
        json!(7),
        json!(null),
        json!(""),
        json!(true),
    ];
    let mut events = vec![start()];
    for (index, id) in (0_u32..).zip(&ids) {
        events.extend([opened(index, &call(id, "clock")), closed(index)]);
    }
    events.extend(stopped("tool_use"));
    let handed: Vec<String> = folded(&events)
        .into_iter()
        .filter_map(|event| match event {
            StreamEvent::ToolCall(call) => Some(call.id),
            StreamEvent::TextDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::Usage(_) => None,
        })
        .collect();
    assert_eq!(handed.len(), ids.len());
    assert_eq!((handed[0].as_str(), handed[2].as_str()), ("toolu_1", "7"));
    assert_eq!(handed[5], "True");
    for minted in [&handed[1], &handed[3], &handed[4]] {
        assert!(minted.starts_with("call_clock_"), "{minted}");
    }
    let distinct: std::collections::BTreeSet<&String> = handed.iter().collect();
    assert_eq!(distinct.len(), ids.len());
}

// A tool the server runs, a block and a delta of a kind the provider does
// not know, and the text a block holds as it opens say nothing; neither
// does anything of an event of another kind.
#[test]
fn what_the_provider_does_not_read_says_nothing() {
    let mut events = vec![
        start(),
        json!({"type": "ping"}),
        json!({"type": "future_event", "text": "Not read."}),
        opened(
            0,
            &json!({"type": "server_tool_use", "id": "srv_1", "name": "web_search"}),
        ),
        delta(0, &fragment("{\"query\": \"x\"}")),
        closed(0),
        opened(1, &json!({"type": "text", "text": "Not read."})),
        delta(1, &json!({"type": "citations_delta", "citation": {}})),
        delta(1, &json!({"type": "text_delta", "text": "Read."})),
        delta(1, &json!({"text": "No type."})),
        closed(1),
        opened(2, &json!({"text": "A block of no type."})),
        closed(2),
    ];
    events.extend(stopped("end_turn"));
    let events = folded(&events);
    let [StreamEvent::TextDelta(text), StreamEvent::Done(done)] = events.as_slice() else {
        panic!("unexpected events: {events:?}");
    };
    assert_eq!(text.text, "Read.");
    assert_eq!(done.finish_reason.as_deref(), Some("end_turn"));
}

// What the folding keeps of a stream is bounded: the calls of a message,
// the blocks it holds open, and each call's id, name and arguments.
#[test]
fn what_the_folding_keeps_is_bounded() {
    let many = |count: u32, block: &Value, close: bool| {
        let mut events = vec![start()];
        for index in 0..count {
            events.push(opened(index, block));
            events.extend(close.then(|| closed(index)));
        }
        events
    };
    let limit = u32::try_from(CALLS_LIMIT).unwrap();
    let clock = call(&json!(null), "clock");
    let text = json!({"type": "text"});
    for (events, why) in [
        (many(limit + 1, &clock, true), "more than 1024 calls"),
        (
            many(limit + 1, &text, false),
            "more than 1024 open content blocks",
        ),
        (
            vec![
                start(),
                opened(0, &call(&json!("c"), &"n".repeat(NAME_LIMIT + 1))),
            ],
            "longer than 1024 bytes",
        ),
        (
            vec![
                start(),
                opened(0, &call(&json!("i".repeat(NAME_LIMIT + 1)), "clock")),
            ],
            "longer than 1024 bytes",
        ),
    ] {
        let (_, failure, _) = fold(&events);
        let error = failure.unwrap();
        assert!(error.message.contains(why), "{}", error.message);
        assert!(!error.retryable);
    }
    // At their bounds they fold: closed blocks are not kept.
    assert!(fold(&many(limit, &clock, true)).1.is_none());
    assert!(fold(&many(4 * limit, &text, true)).1.is_none());
    let longest = "n".repeat(NAME_LIMIT);
    assert!(
        fold(&[start(), opened(0, &call(&json!(longest), &longest))])
            .1
            .is_none()
    );

    let piece = "x".repeat(ARGUMENTS_LIMIT / 4);
    let mut events = vec![start(), opened(0, &call(&json!("c"), "clock"))];
    events.extend((0..4).map(|_| delta(0, &fragment(&piece))));
    assert!(fold(&events).1.is_none());
    events.push(delta(0, &fragment("x")));
    let (ready, failure, _) = fold(&events);
    let error = failure.unwrap();
    assert_eq!(
        error.message,
        "The provider answered with a body it could not read: the arguments of a call are \
         longer than 4194304 bytes"
    );
    // What came before the bound was handed on.
    assert_eq!(ready.len(), 5);
}

// An error a stream tells reads as the status its type names, and one that
// names none is final, whatever its words say.
#[test]
fn an_error_event_fails_as_the_status_it_names() {
    let error = |kind: &str| {
        let told =
            json!({"type": kind, "message": "The server is overloaded: placeholder-not-a-key"});
        fold(&[start(), json!({"type": "error", "error": told})])
            .1
            .unwrap()
    };
    let overloaded = error("overloaded_error");
    assert_eq!(
        (overloaded.status_code, overloaded.retryable),
        (Some(529), true)
    );
    assert_eq!(overloaded.message, "The server is overloaded: [***]");
    let unknown = error("server_busy");
    assert_eq!((unknown.status_code, unknown.retryable), (None, false));
    // An event that names no type and holds an error is the stream's error.
    let typeless = json!({"error": {"type": "rate_limit_error", "message": "Slow down."}});
    let limited = fold(&[typeless]).1.unwrap();
    assert_eq!(
        (
            limited.message.as_str(),
            limited.status_code,
            limited.retryable
        ),
        ("Slow down.", Some(429), true)
    );
}
