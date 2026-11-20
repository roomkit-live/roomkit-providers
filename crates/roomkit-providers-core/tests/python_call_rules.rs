//! The rules on a call's arguments (RFC §6.4) against what `RoomKit`'s Python
//! decides: `fixtures/python_call_rules.json`, written by
//! `scripts/generate_python_forms.py` (`make python-forms`).
//!
//! Each case holds the raw arguments a wire carried for a call, how the
//! response ended and whether the call is its last, with what Python's
//! `unreadable_arguments`, `call_partial`, `call_cut` and `call_garbled`
//! answer. Each argument text holds the mapping Python's `tool_arguments`
//! reads it as.

use roomkit_providers_core::{
    AiToolCall, CallCutOptions, CallGarbledOptions, CallPartialOptions, Metadata, call_cut,
    call_garbled, call_partial, tool_arguments, unreadable_arguments,
};
use serde::Deserialize;
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/python_call_rules.json");

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    arguments: Vec<Read>,
}

#[derive(Debug, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one answer per Python function, as the fixture holds them"
)]
struct Case {
    raw: Value,
    finish_reason: Option<String>,
    last: bool,
    unreadable: bool,
    partial: bool,
    cut: bool,
    garbled: bool,
}

#[derive(Deserialize)]
struct Read {
    raw: String,
    arguments: Metadata,
}

#[test]
fn the_rules_decide_as_python_s_do() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture.cases.len(), 62, "the fixture lost cases");
    for case in &fixture.cases {
        let reason = case.finish_reason.as_deref();
        let last = case.last;
        assert_eq!(
            (
                unreadable_arguments(&case.raw),
                call_partial(&case.raw, reason, CallPartialOptions { last }),
                call_cut(&case.raw, reason, CallCutOptions { last }),
                call_garbled(&case.raw, reason, CallGarbledOptions { last }),
            ),
            (case.unreadable, case.partial, case.cut, case.garbled),
            "{case:?}"
        );
    }
}

/// A provider reading its wire marks a call as the rules decide for its
/// argument text.
#[test]
fn a_call_read_from_its_wire_is_marked_as_the_rules_decide() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    let texts = fixture.cases.iter().filter(|case| case.raw.is_string());
    let mut read = 0;
    for case in texts {
        let raw = case.raw.as_str().unwrap_or_default();
        let reason = case.finish_reason.as_deref();
        let call = AiToolCall::read("call_1", "lookup", raw, reason, case.last);
        assert_eq!(
            (call.partial, call.garbled, call.arguments),
            (case.partial, case.garbled, tool_arguments(raw)),
            "{case:?}"
        );
        read += 1;
    }
    assert_eq!(read, 38, "the fixture lost its argument texts");
}

#[test]
fn an_argument_text_reads_as_the_mapping_python_reads() {
    let fixture: Fixture = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture.arguments.len(), 14, "the fixture lost texts");
    for text in &fixture.arguments {
        assert_eq!(tool_arguments(&text.raw), text.arguments, "{:?}", text.raw);
    }
}
