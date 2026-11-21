//! The composition of a streamed call: the events that name it while its
//! fragments come, and the whole call they end with.

use std::collections::{HashMap, HashSet};

use roomkit_providers_llm::{AiRequest, ProviderError, StreamEvent};
use serde_json::{Value, json};

use super::support::{chunk, ending, events, streamed};

const CASES: &str = include_str!("composition_cases.json");

/// The events `chunks` stream, each event or its failure.
async fn read(chunks: &[Value]) -> Vec<Result<StreamEvent, ProviderError>> {
    let (events, failure) = streamed(events(chunks), &AiRequest::default()).await;
    let events = events.into_iter().map(Ok);
    events.chain(failure.map(Err)).collect()
}

// Source: tests/agent/composition.rs::wire_fragments_keep_each_calls_final_id
// Python: tests/test_providers/test_openai_composition.py
#[tokio::test]
async fn wire_fragments_keep_each_calls_final_id() {
    let cases: Vec<Value> = serde_json::from_str(CASES).unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let fragments = case[1].as_array().unwrap().iter();
        let mut chunks: Vec<Value> = fragments
            .map(|fragment| chunk(&json!({"tool_calls": [fragment]})))
            .collect();
        chunks.extend(ending("tool_calls", 1, 1));
        let events = read(&chunks).await.into_iter().map(Result::unwrap);
        let events: Vec<StreamEvent> = events.collect();
        let deltas = events.iter().filter_map(|event| match event {
            StreamEvent::ToolCallDelta(delta) => Some(delta),
            StreamEvent::TextDelta(_)
            | StreamEvent::ToolCall(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::Usage(_) => None,
        });
        let deltas: Vec<_> = deltas.collect();
        let calls = events.iter().filter_map(|event| match event {
            StreamEvent::ToolCall(call) => Some(call),
            StreamEvent::TextDelta(_)
            | StreamEvent::Done(_)
            | StreamEvent::ThinkingDelta(_)
            | StreamEvent::ToolCallDelta(_)
            | StreamEvent::Usage(_) => None,
        });
        let calls: Vec<_> = calls.collect();
        let composed = deltas.iter();
        let composed = composed.map(|delta| (&delta.index, &delta.name, &delta.arguments_delta));
        assert_eq!(json!(composed.collect::<Vec<_>>()), case[2], "{}", case[0]);
        let arguments = calls.iter().map(|call| &call.arguments);
        assert_eq!(json!(arguments.collect::<Vec<_>>()), case[3], "{}", case[0]);
        // Each call has an id of its own, and the events of one call name
        // it by the id it ends with.
        let ids: HashSet<_> = calls.iter().map(|call| &call.id).collect();
        assert_eq!(ids.len(), calls.len(), "{}", case[0]);
        let mut by_slot = HashMap::new();
        for delta in deltas {
            assert_eq!(by_slot.entry(delta.index).or_insert(&delta.id), &&delta.id);
            let whole = calls.iter();
            let mut whole = whole.filter(|call| call.id == delta.id && call.name == delta.name);
            assert!(whole.next().is_some(), "{}", case[0]);
        }
        let named: HashSet<_> = by_slot.values().collect();
        assert_eq!(named.len(), by_slot.len(), "{}", case[0]);
        // Every composition event comes before the first whole call.
        let is_call = |event: &StreamEvent| matches!(event, StreamEvent::ToolCall(_));
        let first_call = events.iter().position(is_call);
        let composition = events.iter().enumerate();
        let mut late = composition
            .filter(|(at, event)| {
                matches!(event, StreamEvent::ToolCallDelta(_)) && Some(*at) >= first_call
            })
            .map(|(at, _)| at);
        assert_eq!(late.next(), None, "{}", case[0]);
        assert!(matches!(events.last(), Some(StreamEvent::Done(_))));
        match case[0].as_str().unwrap() {
            "late-id-after-name" => assert!(calls[0].id.starts_with("call_search_")),
            "late-id-before-name" => assert_eq!(calls[0].id, "late"),
            _ => {}
        }
    }
}

// Source: tests/agent/composition.rs::composition_precedes_reasoning_and_text_in_the_same_chunk
#[tokio::test]
async fn composition_precedes_reasoning_and_text_in_the_same_chunk() {
    let chunks = [chunk(&json!({
        "tool_calls": [{"index": 0, "id": "a", "function": {"name": "search", "arguments": "{}"}}],
        "reasoning_content": "checking", "content": "answer"
    }))];
    let events = read(&chunks).await.into_iter().map(Result::unwrap);
    let events: Vec<StreamEvent> = events.collect();
    assert!(matches!(
        events.as_slice(),
        [
            StreamEvent::ToolCallDelta(_),
            StreamEvent::ThinkingDelta(_),
            StreamEvent::TextDelta(_),
            StreamEvent::ToolCall(_),
            StreamEvent::Done(_)
        ]
    ));
}

// Source: tests/agent/composition.rs::a_failed_wire_keeps_its_fragment_without_a_complete_call
#[tokio::test]
async fn a_failed_wire_keeps_its_fragment_without_a_complete_call() {
    let chunks = [
        chunk(&json!({"tool_calls": [{"index": 0, "id": "a",
            "function": {"name": "search", "arguments": "{secret"}}]})),
        json!({"error": {"message": "failed", "code": "invalid_request_error"}}),
    ];
    let events = read(&chunks).await;
    assert_eq!(events.len(), 2);
    assert!(
        matches!(&events[0], Ok(StreamEvent::ToolCallDelta(d)) if d.arguments_delta == "{secret")
    );
    let error = events[1].as_ref().unwrap_err();
    assert_eq!(
        (error.message.as_str(), error.status_code, error.retryable),
        ("failed", Some(400), false)
    );
}

// Source: tests/agent/composition.rs::a_later_malformed_call_keeps_the_first_fragment_of_its_chunk
#[tokio::test]
async fn a_later_malformed_call_keeps_the_first_fragment_of_its_chunk() {
    for fault in ["id", "name", "content"] {
        let mut delta = json!({"tool_calls": [
            {"index": 0, "id": "a", "function": {"name": "search", "arguments": "{}"}}
        ]});
        if fault == "content" {
            delta["content"] = 7.into();
        } else {
            let mut malformed =
                json!({"index": 1, "id": "b", "function": {"name": "search", "arguments": "{}"}});
            if fault == "id" {
                malformed["id"] = 7.into();
            } else {
                malformed["function"]["name"] = 7.into();
            }
            delta["tool_calls"].as_array_mut().unwrap().push(malformed);
        }
        let events = read(&[chunk(&delta)]).await;
        assert_eq!(events.len(), 2, "{fault}");
        assert!(
            matches!(&events[0], Ok(StreamEvent::ToolCallDelta(d)) if d.id == "a" && d.arguments_delta == "{}")
        );
        let error = events[1].as_ref().unwrap_err();
        assert_eq!(
            (error.retryable, error.status_code),
            (false, None),
            "{fault}"
        );
    }
}

// Source: tests/agent/composition.rs::a_cut_call_keeps_its_fragments_and_partial_arguments
#[tokio::test]
async fn a_cut_call_keeps_its_fragments_and_partial_arguments() {
    let mut chunks = vec![chunk(&json!({"tool_calls": [
        {"index": 0, "function": {"name": "search", "arguments": "{\"q\":"}}
    ]}))];
    chunks.extend(ending("length", 1, 1));
    let events = read(&chunks).await.into_iter().map(Result::unwrap);
    let events: Vec<StreamEvent> = events.collect();
    let [
        StreamEvent::ToolCallDelta(delta),
        StreamEvent::ToolCall(call),
        StreamEvent::Done(done),
    ] = events.as_slice()
    else {
        panic!("unexpected events: {events:?}");
    };
    assert_eq!(delta.id, call.id);
    assert!(call.partial);
    assert_eq!(json!(call.arguments), json!({"raw": "{\"q\":"}));
    assert_eq!(done.finish_reason.as_deref(), Some("length"));
}
