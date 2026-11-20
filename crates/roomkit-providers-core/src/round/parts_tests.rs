use super::*;
use crate::response::StreamToolCall;

// Python: tests/test_providers/test_thinking_blocks.py::test_replaying_a_call_the_round_never_made_is_an_error
#[test]
fn replaying_a_call_the_round_never_made_is_an_error() {
    let mut transcript = RoundTranscript::new();
    transcript.add_thinking(&StreamThinkingDelta {
        block: Some(0),
        signature: Some("S0".to_owned()),
        ..StreamThinkingDelta::new("a")
    });
    let error = transcript
        .parts(&[AiToolCall::new("ghost", "x"), AiToolCall::new("b", "x")])
        .expect_err("calls the round never made");
    assert_eq!(
        error.to_string(),
        "replaying calls the round never made: ['b', 'ghost']"
    );
}

// A call past the round's bound stays out, the transcript keeping its
// place alone (Python's `_cap_round_tool_calls` before `parts`).
#[test]
fn a_call_it_made_and_the_loop_dropped_stays_out() {
    let mut transcript = RoundTranscript::new();
    transcript.add_thinking(&StreamThinkingDelta {
        block: Some(0),
        signature: Some("S0".to_owned()),
        ..StreamThinkingDelta::new("a")
    });
    transcript.add_call("kept");
    transcript.add_call("dropped");
    let parts = transcript
        .parts(&[AiToolCall::new("kept", "x")])
        .expect("the round made it");
    assert_eq!(parts.len(), 2);
    assert!(matches!(&parts[1], AiContentPart::ToolCall(call) if call.id == "kept"));
}

// Python's `RoundCall`: a stream's call or a response's, its metadata kept
// either way (Python: tests/test_providers/test_thinking_blocks.py reads
// both through `call_part`).
#[test]
fn a_round_takes_a_stream_s_call_or_a_response_s() {
    let metadata = serde_json::json!({ "thought_signature": "TS" });
    let metadata = metadata.as_object().cloned().unwrap_or_default();
    let streamed = StreamToolCall {
        metadata: metadata.clone(),
        ..StreamToolCall::new("c1", "x")
    };
    let parts = round_parts(&[], "said", std::slice::from_ref(&streamed));
    assert!(matches!(
        &parts[1],
        AiContentPart::ToolCall(part) if part.metadata == metadata
    ));
    let mut transcript = RoundTranscript::new();
    transcript.add_thinking(&StreamThinkingDelta {
        block: Some(0),
        signature: Some("S0".to_owned()),
        ..StreamThinkingDelta::new("a")
    });
    transcript.add_call("c1");
    let keyed = transcript.parts(&[streamed]).expect("the round made it");
    assert!(matches!(
        &keyed[1],
        AiContentPart::ToolCall(part) if part.metadata == metadata
    ));
    let answered = transcript.parts::<AiToolCall>(&[]).expect("an answer");
    assert_eq!(answered.len(), 1);
    assert_eq!(transcript.thinking().last_signature(), Some("S0"));
}

// A call's intent goes back among its arguments (RFC §6.4).
#[test]
fn a_call_part_writes_its_intent_back() {
    let call = AiToolCall {
        intent: Some("Look it up.".to_owned()),
        ..AiToolCall::new("c1", "lookup")
    };
    let parts = round_parts(&[], "", &[call]);
    assert!(matches!(
        &parts[0],
        AiContentPart::ToolCall(part)
            if part.arguments.get(INTENT_PROPERTY).and_then(Value::as_str) == Some("Look it up.")
    ));
}

fn delta(block: Option<u32>, thinking: &str) -> StreamThinkingDelta {
    StreamThinkingDelta {
        block,
        ..StreamThinkingDelta::new(thinking)
    }
}

fn signed(block: Option<u32>, thinking: &str, signature: &str) -> StreamThinkingDelta {
    StreamThinkingDelta {
        signature: Some(signature.to_owned()),
        ..delta(block, thinking)
    }
}

fn thinking(text: &str, signature: Option<&str>) -> AiContentPart {
    AiContentPart::Thinking(AiThinkingPart {
        signature: signature.map(str::to_owned),
        ..AiThinkingPart::new(text)
    })
}

fn text(said: &str) -> AiContentPart {
    AiContentPart::Text(AiTextPart::new(said))
}

fn call(id: &str, name: &str) -> AiContentPart {
    AiContentPart::ToolCall(AiToolCallPart::new(id, name))
}

// Python: RoundTranscript.parts without named blocks: the reasoning, then
// the text, then the calls, wherever each came in the round.
#[test]
fn an_unkeyed_round_replays_its_reasoning_then_its_text_then_its_calls() {
    let mut transcript = RoundTranscript::new();
    transcript.add_text("Let me ");
    transcript.add_call("c1");
    transcript.add_thinking(&delta(None, "Look it up."));
    transcript.add_text("check.");
    transcript.add_call("c2");
    let calls = [
        AiToolCall::new("c1", "lookup"),
        AiToolCall::new("c2", "search"),
    ];
    assert_eq!(
        transcript.parts(&calls).unwrap(),
        [
            thinking("Look it up.", None),
            text("Let me check."),
            call("c1", "lookup"),
            call("c2", "search"),
        ]
    );
    assert_eq!(transcript.text(), "Let me check.");
    // A round that said nothing replays no text part.
    let mut silent = RoundTranscript::new();
    silent.add_text("");
    silent.add_call("c1");
    assert_eq!(
        silent.parts(&[AiToolCall::new("c1", "lookup")]).unwrap(),
        [call("c1", "lookup")]
    );
}

// Python: tests/test_providers/test_thinking_blocks.py: a round whose vendor
// named its blocks goes back block by block, its text and its calls where
// they came.
#[test]
fn a_keyed_round_replays_each_block_its_text_and_its_calls_where_they_came() {
    let mut transcript = RoundTranscript::new();
    transcript.add_thinking(&delta(Some(0), "fir"));
    transcript.add_thinking(&signed(Some(0), "st", "S0"));
    transcript.add_text("One, ");
    transcript.add_text("two.");
    transcript.add_call("c1");
    transcript.add_thinking(&signed(Some(1), "second", "S1"));
    transcript.add_text("");
    transcript.add_text("Three.");
    transcript.add_call("c2");
    let calls = [
        AiToolCall::new("c2", "search"),
        AiToolCall::new("c1", "lookup"),
    ];
    assert_eq!(
        transcript.parts(&calls).unwrap(),
        [
            thinking("first", Some("S0")),
            text("One, two."),
            call("c1", "lookup"),
            thinking("second", Some("S1")),
            text("Three."),
            call("c2", "search"),
        ]
    );
    assert_eq!(transcript.text(), "One, two.Three.");
}

// Python: `_taken`: two calls under one id each keep their own place (RFC
// §6.4), in the order they were made.
#[test]
fn two_calls_under_one_id_each_keep_their_place() {
    let mut transcript = RoundTranscript::new();
    transcript.add_thinking(&signed(Some(0), "a", "S0"));
    transcript.add_call("c1");
    transcript.add_text("then");
    transcript.add_call("c1");
    let calls = [
        AiToolCall::new("c1", "first"),
        AiToolCall::new("c1", "second"),
    ];
    assert_eq!(
        transcript.parts(&calls).unwrap(),
        [
            thinking("a", Some("S0")),
            call("c1", "first"),
            text("then"),
            call("c1", "second"),
        ]
    );
    // A third call under the id is one the round never made.
    let more = [
        calls[0].clone(),
        calls[1].clone(),
        AiToolCall::new("c1", "third"),
    ];
    assert!(transcript.parts(&more).is_err());
}
