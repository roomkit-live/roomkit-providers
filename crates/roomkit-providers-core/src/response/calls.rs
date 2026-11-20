//! The calls a response makes, and the one reading of a call's id and
//! arguments every provider applies (RFC §6.4).

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::Metadata;

/// The endings under which a response ran out of room: the output cap, the
/// context window filling up mid-answer, in each provider's word.
const OUT_OF_ROOM: [&str; 4] = [
    "length",
    "max_tokens",
    "model_context_window_exceeded",
    "model_length",
];

/// The outcome of a call already executed by the provider (RFC §9.3).
/// Python's `ServedCall(*, result="", is_error=False)`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct ServedCall {
    /// The original result, reported whole before its model copy is bounded.
    pub result: String,
    /// Whether the provider reported a failed call.
    pub is_error: bool,
}

/// A call the model made (RFC §6.4 "What a provider hands the loop for a
/// call"): Python's `AIToolCall(*, id, name, arguments={}, metadata={},
/// served=None, partial=False, garbled=False, intent=None)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AiToolCall {
    /// The wire id. Pending calls use a unique id ([`CallIds`]); provider-served
    /// calls sharing an id are each reported as their own occurrence (RFC §12.4).
    pub id: String,
    /// The tool called.
    pub name: String,
    /// Its arguments, a mapping ([`tool_arguments`]).
    pub arguments: Metadata,
    /// Provider-owned data of the call.
    pub metadata: Metadata,
    /// An outcome the provider already produced; `None` leaves the call pending.
    pub served: Option<ServedCall>,
    /// The call never runs: its arguments do not read, or the response cut
    /// them short.
    pub partial: bool,
    /// The model wrote the arguments unreadable, where the response did not
    /// cut them: a `partial` call the model is told to write again.
    pub garbled: bool,
    /// What the call does, in the sentence the model wrote for the person
    /// (RFC §6.4); a provider leaves it `None`.
    pub intent: Option<String>,
}

impl AiToolCall {
    /// The call `id` of the tool `name`, with no arguments.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: Metadata::new(),
            metadata: Metadata::new(),
            served: None,
            partial: false,
            garbled: false,
            intent: None,
        }
    }
}

/// Hands each call of one response its id: the server's when it gave one no
/// earlier call of the response took, a minted one otherwise (`call_<name>_<12
/// hex digits>`, unique across turns). Results, cuts and a call's rows all find
/// a call by its id, so a provider reading its wire gives two calls two ids
/// (RFC §6.4). Python's `CallIds`.
///
/// ```
/// use roomkit_providers_core::CallIds;
///
/// let mut ids = CallIds::new();
/// assert_eq!(ids.call("c1", "now"), "c1");
/// assert!(ids.call("c1", "now").starts_with("call_now_"));
/// assert!(ids.call("", "now").starts_with("call_now_"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CallIds {
    taken: HashSet<String>,
}

impl CallIds {
    /// No id taken yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The id of the response's next call, to the tool `name`, its server's
    /// `server_id` empty when it gave none. Python's `ids(server_id, name)`:
    /// Rust has no object to call.
    pub fn call(&mut self, server_id: &str, name: &str) -> String {
        let id = if !server_id.is_empty() && !self.taken.contains(server_id) {
            server_id.to_owned()
        } else {
            minted_call_id(name)
        };
        self.taken.insert(id.clone());
        id
    }
}

/// An id for a call to `name` its server gave none, unique across turns:
/// Python's `minted_call_id`.
fn minted_call_id(name: &str) -> String {
    // The 48 high bits of a v4 UUID are random: uuid4().hex[:12].
    let random = uuid::Uuid::new_v4().as_u128() >> 80;
    format!("call_{name}_{random:012x}")
}

/// A call's arguments as a mapping, never an error (RFC §6.4): Python's
/// `tool_arguments(raw)`. No arguments (nothing, blank text, JSON `null`)
/// are `{}`; text that parses to a JSON object is that object; anything
/// else (invalid JSON, an array, a fragment the output cap cut) is kept
/// whole under `raw`.
///
/// ```
/// use roomkit_providers_core::tool_arguments;
/// use serde_json::json;
///
/// assert_eq!(json!(tool_arguments(r#"{"order": "42"}"#)), json!({ "order": "42" }));
/// assert_eq!(json!(tool_arguments(" null ")), json!({}));
/// assert_eq!(json!(tool_arguments("[1, 2]")), json!({ "raw": "[1, 2]" }));
/// ```
pub fn tool_arguments(raw: &str) -> Metadata {
    arguments_or_raw(raw, read_arguments(raw))
}

/// `read`, the mapping `raw` reads as, or `raw` kept whole under `raw` when
/// it reads as none.
pub(crate) fn arguments_or_raw(raw: &str, read: Option<Metadata>) -> Metadata {
    read.unwrap_or_else(|| {
        let mut kept = Metadata::new();
        kept.insert("raw".to_owned(), Value::String(raw.to_owned()));
        kept
    })
}

/// `raw` as a mapping, `None` when it does not read as one.
pub(crate) fn read_arguments(raw: &str) -> Option<Metadata> {
    if blank(raw) {
        return Some(Metadata::new());
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(arguments)) => Some(arguments),
        Ok(Value::Null) => Some(Metadata::new()),
        Ok(_) | Err(_) => None,
    }
}

/// Whether `text` is nothing but space as Python's `str.strip()` reads it,
/// which also treats the four ASCII separators as space.
pub(crate) fn blank(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
}

/// Whether a response that ended for `finish_reason` ran out of room: its
/// output cap, or its context window filling up mid-answer, whatever its
/// provider calls it and in any case. Python's `is_truncation`.
pub fn is_truncation(finish_reason: Option<&str>) -> bool {
    finish_reason.is_some_and(|reason| OUT_OF_ROOM.contains(&reason.to_ascii_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Python: tests/test_providers/test_tool_call_handoff.py::TestCallIds::test_every_call_of_a_response_gets_its_own_id
    #[test]
    fn every_call_of_a_response_gets_its_own_id() {
        let mut ids = CallIds::new();
        let handed = ["c1", "", "", "c1"].map(|id| ids.call(id, "now"));
        let distinct: HashSet<&String> = handed.iter().collect();
        let minted = handed.get(1..).unwrap_or_default();
        assert_eq!(
            (handed.first().map(String::as_str), distinct.len()),
            (Some("c1"), 4)
        );
        assert!(
            minted.iter().all(|id| id.starts_with("call_now_")
                && id.len() == "call_now_".len() + 12
                && id.bytes().skip(9).all(|byte| byte.is_ascii_hexdigit())),
            "{handed:?}"
        );
    }

    /// RFC §6.4: arguments reach the loop as a mapping, never as an error.
    #[test]
    fn arguments_read_as_a_mapping_whatever_the_wire_carried() {
        let read = [
            r#"{"order": "42"}"#,
            "",
            " \u{1f}\n",
            "null",
            "[1, 2]",
            r#"{"order": "#,
            "NaN",
        ]
        .map(|raw| Value::Object(tool_arguments(raw)));
        assert_eq!(
            read,
            [
                json!({ "order": "42" }),
                json!({}),
                json!({}),
                json!({}),
                json!({ "raw": "[1, 2]" }),
                json!({ "raw": r#"{"order": "# }),
                json!({ "raw": "NaN" }),
            ]
        );
    }

    #[test]
    fn a_response_out_of_room_is_told_in_every_provider_s_word() {
        for reason in [
            "length",
            "max_tokens",
            "MAX_TOKENS",
            "model_length",
            "model_context_window_exceeded",
            "MODEL_LENGTH",
        ] {
            assert!(is_truncation(Some(reason)), "{reason}");
        }
        for reason in [
            Some("stop"),
            Some("tool_calls"),
            Some("content_filter"),
            None,
        ] {
            assert!(!is_truncation(reason), "{reason:?}");
        }
    }
}
