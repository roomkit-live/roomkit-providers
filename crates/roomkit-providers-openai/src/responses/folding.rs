//! Responses events: text arrives immediately, calls wait for a terminal snapshot.

use roomkit_ai::{CallIds, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta};
use serde_json::Value;

use super::wire;
use crate::failure::Failure;
use roomkit_providers_client::chat::call_slots::RawCall;

#[derive(Default)]
pub(super) struct Folding {
    text: String,
    /// Whether the reasoning came as it was written: the terminal response
    /// then has nothing to add of it.
    reasoned: bool,
    items: super::items::Items,
    pub(super) namespace: Option<String>,
}

impl Folding {
    /// Reads the data of one event, adds to `ready` what it gives, and says
    /// whether it ended the answer.
    pub(super) fn take(
        &mut self,
        data: &str,
        ready: &mut Vec<StreamEvent>,
    ) -> Result<bool, Failure> {
        if data == "[DONE]" {
            return Err(wire::malformed(
                "Responses stream ended without a terminal response",
            ));
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|_| wire::malformed("invalid Responses event JSON"))?;
        match value.get("type").and_then(Value::as_str) {
            Some("response.output_item.added") => self.items.record(&value, false)?,
            Some("response.output_item.done") => self.items.record(&value, true)?,
            Some("response.output_text.delta" | "response.refusal.delta") => {
                let delta = value
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or_else(|| wire::malformed("Responses text delta is not text"))?;
                self.text.push_str(delta);
                push_text(ready, delta.to_owned());
            }
            Some("response.reasoning_summary_text.delta" | "response.reasoning_text.delta") => {
                // Nothing to read in one that is no text or empty: the
                // terminal response's reasoning then stands.
                let delta = value.get("delta").and_then(Value::as_str);
                if let Some(delta) = delta.filter(|delta| !delta.is_empty()) {
                    self.reasoned = true;
                    push_thinking(ready, delta.to_owned());
                }
            }
            Some("response.completed" | "response.incomplete") => {
                let mut response = value
                    .get("response")
                    .ok_or_else(|| wire::malformed("terminal Responses event has no response"))?
                    .clone();
                let expected = value
                    .get("type")
                    .and_then(Value::as_str)
                    .and_then(|kind| kind.strip_prefix("response."));
                if response.get("status").and_then(Value::as_str) != expected {
                    return Err(wire::malformed(
                        "Responses terminal event and status disagree",
                    ));
                }
                self.items.finish(&mut response)?;
                wire::validate_namespace(&response, self.namespace.as_deref())?;
                let whole = wire::response(&response)?;
                let suffix = whole.text.strip_prefix(self.text.as_str()).ok_or_else(|| {
                    wire::malformed("Responses terminal text differs from its streamed text")
                })?;
                if !self.reasoned {
                    push_thinking(ready, whole.thinking.clone().unwrap_or_default());
                }
                if !suffix.is_empty() {
                    push_text(ready, suffix.to_owned());
                }
                push_end(ready, whole.calls, whole.done);
                return Ok(true);
            }
            Some("response.failed" | "response.cancelled") => {
                return Err(wire::failed(value.get("response").unwrap_or(&value)));
            }
            Some("error") => return Err(wire::failed(&value)),
            Some(_) => {} // Lifecycle/composition events; the terminal snapshot owns calls.
            None => return Err(wire::malformed("Responses event has no type")),
        }
        Ok(false)
    }
}

/// Queues `thinking`, when there is any, as a reasoning event.
pub(super) fn push_thinking(ready: &mut Vec<StreamEvent>, thinking: String) {
    if !thinking.is_empty() {
        ready.push(StreamEvent::ThinkingDelta(StreamThinkingDelta::new(
            thinking,
        )));
    }
}

/// Queues `text`, when there is any, as a text event.
pub(super) fn push_text(ready: &mut Vec<StreamEvent>, text: String) {
    if !text.is_empty() {
        ready.push(StreamEvent::TextDelta(StreamTextDelta::new(text)));
    }
}

/// Queues the end of an answer: its calls, each under an id of its own,
/// read for its ending as every provider reads them (RFC §6.4), then its
/// done event.
pub(super) fn push_end(ready: &mut Vec<StreamEvent>, calls: Vec<RawCall>, done: StreamDone) {
    let mut ids = CallIds::new();
    let calls = calls.into_iter().map(|call| {
        let id = ids.call(&call.id, &call.name);
        RawCall::new(id, call.name, call.arguments)
    });
    crate::folding::push_end(ready, calls.collect(), done);
}
