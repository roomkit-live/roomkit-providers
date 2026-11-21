//! A provider's decisions on a call's arguments, as received from the wire
//! (RFC §6.4): whether they read, and whether the response cut them short.

use serde_json::Value;

use crate::response::{AiToolCall, arguments_or_raw, blank, is_truncation, read_arguments};

/// The endings beside running out of room that can stop a call
/// mid-arguments: a generation error, a content filter, a refusal.
const CUTTING: [&str; 3] = ["error", "content_filter", "refusal"];

/// The endings of a response whose call its provider did not hand over:
/// one it could not parse, or one to a tool the request did not enable.
const MALFORMED_CALL: [&str; 2] = ["malformed_function_call", "unexpected_tool_call"];

// Each function has its own options type, as Python gives each its own
// `last=True`. All three read the same position within a round of calls.
macro_rules! position_options {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name {
            /// Whether this is the last call in the provider response.
            pub last: bool,
        }

        impl Default for $name {
            fn default() -> Self {
                Self { last: true }
            }
        }
    };
}

position_options!(
    CallCutOptions,
    "Position of the call checked by [`call_cut`]: Python's `last=True`."
);
position_options!(
    CallPartialOptions,
    "Position of the call checked by [`call_partial`]: Python's `last=True`."
);
position_options!(
    CallGarbledOptions,
    "Position of the call checked by [`call_garbled`]: Python's `last=True`."
);

/// Whether a call's raw arguments do not read as an object, which makes the
/// call partial: Python's `unreadable_arguments(raw)`. A mapping, no
/// arguments, blank text and JSON `null` read.
pub fn unreadable_arguments(raw: &Value) -> bool {
    match raw {
        Value::Object(_) | Value::Null => false,
        Value::String(text) => read_arguments(text).is_none(),
        Value::Bool(_) | Value::Number(_) | Value::Array(_) => true,
    }
}

/// Whether the response was cut short over a call's arguments: Python's
/// `call_cut(raw, finish_reason, *, last=True)`. It is the response's last
/// call, the response ended on something that stops a call mid-arguments,
/// and they did not arrive whole.
pub fn call_cut(raw: &Value, finish_reason: Option<&str>, options: CallCutOptions) -> bool {
    ArgumentStatus::of(raw, finish_reason, options.last).cut
}

/// Whether a call must not run: Python's `call_partial(raw, finish_reason,
/// *, last=True)`. Its arguments do not read, or the response was cut over
/// it before argument text that reads arrived.
pub fn call_partial(raw: &Value, finish_reason: Option<&str>, options: CallPartialOptions) -> bool {
    ArgumentStatus::of(raw, finish_reason, options.last).partial
}

/// Whether the model wrote a call's arguments unreadable: Python's
/// `call_garbled(raw, finish_reason, *, last=True)`. They do not read, and
/// the response did not cut them short.
pub fn call_garbled(raw: &Value, finish_reason: Option<&str>, options: CallGarbledOptions) -> bool {
    ArgumentStatus::of(raw, finish_reason, options.last).garbled
}

/// What a call's arguments make of the call.
struct ArgumentStatus {
    partial: bool,
    garbled: bool,
    cut: bool,
}

impl ArgumentStatus {
    fn of(raw: &Value, finish_reason: Option<&str>, last: bool) -> Self {
        let text_arrived = raw.as_str().is_some_and(|text| !blank(text));
        Self::from_read(unreadable_arguments(raw), text_arrived, finish_reason, last)
    }

    fn from_read(
        unreadable: bool,
        text_arrived: bool,
        finish_reason: Option<&str>,
        last: bool,
    ) -> Self {
        let cut = last && cuts_calls(finish_reason) && (unreadable || !text_arrived);
        Self {
            partial: unreadable || cut,
            garbled: unreadable && !cut,
            cut,
        }
    }
}

/// Whether a response that ended for `finish_reason` may have stopped its
/// last call mid-arguments: it ran out of room, a filter or a refusal
/// stopped it, or its stream ended with no reason at all.
fn cuts_calls(finish_reason: Option<&str>) -> bool {
    finish_reason.is_none_or(|reason| {
        is_truncation(Some(reason)) || CUTTING.contains(&reason.to_ascii_lowercase().as_str())
    })
}

/// Whether a response ended on a call its provider did not hand over:
/// Python's `is_malformed_call`.
pub(crate) fn is_malformed_call(finish_reason: Option<&str>) -> bool {
    finish_reason.is_some_and(|reason| {
        MALFORMED_CALL
            .iter()
            .any(|malformed| reason.eq_ignore_ascii_case(malformed))
    })
}

impl AiToolCall {
    /// The call `id` of `name`, its arguments the text `raw` the wire
    /// carried, read by Python's rules for a response that ended for
    /// `finish_reason`, `last` when it is the response's last call: its
    /// arguments a mapping ([`tool_arguments`](crate::tool_arguments)),
    /// `partial` when they do not read or the response cut them short,
    /// `garbled` when they do not read and nothing cut them. For a provider
    /// reading its wire.
    #[doc(hidden)]
    pub fn read(
        id: impl Into<String>,
        name: impl Into<String>,
        raw: &str,
        finish_reason: Option<&str>,
        last: bool,
    ) -> Self {
        let read = read_arguments(raw);
        let status = ArgumentStatus::from_read(read.is_none(), !blank(raw), finish_reason, last);
        Self {
            arguments: arguments_or_raw(raw, read),
            partial: status.partial,
            garbled: status.garbled,
            ..Self::new(id, name)
        }
    }
}

/// The calls `raw`, each its id, its tool and its argument text, of a
/// response that ended for `finish_reason`, read by [`AiToolCall::read`].
/// For a provider reading its wire.
#[doc(hidden)]
pub fn read_tool_calls(
    raw: Vec<(String, String, String)>,
    finish_reason: Option<&str>,
) -> Vec<AiToolCall> {
    let last = raw.len().checked_sub(1);
    raw.into_iter()
        .enumerate()
        .map(|(position, (id, name, arguments))| {
            AiToolCall::read(id, name, &arguments, finish_reason, Some(position) == last)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn read(calls: &[(&str, &str)], finish_reason: &str) -> Vec<(String, Value, bool, bool)> {
        let raw = calls
            .iter()
            .map(|(id, arguments)| {
                (
                    (*id).to_owned(),
                    "lookup".to_owned(),
                    (*arguments).to_owned(),
                )
            })
            .collect();
        read_tool_calls(raw, Some(finish_reason))
            .into_iter()
            .map(|call| {
                (
                    call.id,
                    Value::Object(call.arguments),
                    call.partial,
                    call.garbled,
                )
            })
            .collect()
    }

    /// RFC §6.4: arguments reach the loop as a mapping, never as an error;
    /// arguments that do not read make a garbled call.
    #[test]
    fn arguments_read_as_a_mapping_whatever_the_wire_carried() {
        let calls = [
            ("a", r#"{"order": "42"}"#),
            ("b", ""),
            ("c", "null"),
            ("d", "[1, 2]"),
        ];
        assert_eq!(
            read(&calls, "tool_calls"),
            vec![
                ("a".to_owned(), json!({ "order": "42" }), false, false),
                ("b".to_owned(), json!({}), false, false),
                ("c".to_owned(), json!({}), false, false),
                ("d".to_owned(), json!({ "raw": "[1, 2]" }), true, true),
            ]
        );
    }

    /// RFC §6.4: only the last call can be cut, and a call whose arguments
    /// arrived and read runs, the response cut over it or not.
    #[test]
    fn only_the_last_call_of_a_cut_response_with_no_argument_text_is_cut() {
        let calls = [("a", ""), ("b", r#"{"order": "1"}"#), ("c", "")];
        let marks: Vec<(bool, bool)> = read(&calls, "length")
            .into_iter()
            .map(|(_, _, partial, garbled)| (partial, garbled))
            .collect();
        assert_eq!(marks, [(false, false), (false, false), (true, false)]);
    }

    #[test]
    fn non_finite_numbers_are_kept_verbatim_by_the_wire_reader() {
        for value in ["NaN", "Infinity", "-Infinity", "1e900", "-1e900"] {
            let raw = format!(r#"{{"record":[{{"amount":{value}}}]}}"#);
            let call = AiToolCall::read("id", "find", &raw, Some("stop"), true);
            assert_eq!(call.arguments["raw"], raw);
            assert!(call.partial && call.garbled);
        }
    }

    // Python: call_cut, call_partial and call_garbled take `last=True` by
    // default: a call is its response's last unless told otherwise.
    #[test]
    fn a_call_is_the_last_of_its_response_by_default() {
        assert!(CallCutOptions::default().last);
        assert!(CallPartialOptions::default().last);
        assert!(CallGarbledOptions::default().last);
        let nothing = json!("");
        assert!(call_cut(
            &nothing,
            Some("length"),
            CallCutOptions::default()
        ));
        assert!(!call_cut(
            &nothing,
            Some("length"),
            CallCutOptions { last: false }
        ));
        assert!(call_partial(
            &nothing,
            Some("length"),
            CallPartialOptions::default()
        ));
        assert!(!call_garbled(
            &json!("[1]"),
            Some("length"),
            CallGarbledOptions::default()
        ));
        assert!(call_garbled(
            &json!("[1]"),
            Some("length"),
            CallGarbledOptions { last: false }
        ));
    }

    // Python: is_malformed_call, the endings of a call a provider did not
    // hand over, in any case.
    #[test]
    fn a_call_a_provider_did_not_hand_over_is_told_by_its_ending() {
        for reason in ["MALFORMED_FUNCTION_CALL", "unexpected_tool_call"] {
            assert!(is_malformed_call(Some(reason)), "{reason}");
        }
        for reason in [Some("stop"), Some("tool_calls"), None] {
            assert!(!is_malformed_call(reason), "{reason:?}");
        }
    }
}
