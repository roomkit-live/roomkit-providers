//! A streamed message folded as its events come: what Python's
//! `stream_events` and `ToolUseBlocks` read of each event, in its order.
//!
//! A message starts, opens content blocks that take deltas and close, says
//! why it stopped and what it used, and ends. A block of text gives its
//! text, a block of reasoning its text and its signature, each naming its
//! block, a redacted one its opaque data; a block that calls a tool tells
//! the call as it opens, each fragment of its arguments, and the call
//! itself when it closes. A call whose block never closed is handed once
//! the message is over, read for how the message stopped.
//!
//! An event is read by the `type` its data names. One the provider does not
//! know says nothing: a ping, a block or a delta of another kind, a tool
//! the server runs. What cannot be folded fails the stream: an event that
//! is no object, content before the message starts, a delta or a closing
//! that names no open block, a text or a reasoning delta in a block of
//! another kind.
//!
//! What the folding keeps of a stream is bounded: its open blocks, its
//! calls, and each call's id, name and arguments. The text and the
//! reasoning are handed on as they come and kept nowhere.

mod calls;
mod usage;

use std::collections::BTreeMap;

use roomkit_providers_llm::{
    Metadata, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta, StreamUsage,
};
use serde_json::Value;

use self::calls::Calls;
use self::usage::Usage;
use crate::failure::{Failure, said_something};

#[cfg(test)]
mod tests;

/// The most content blocks a message holds open at once.
const OPEN_BLOCKS_LIMIT: usize = 1024;

/// What a content block holds, as far as its deltas are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Thinking,
    /// A call of one of the request's tools.
    Call,
    /// A block the provider reads no delta of: redacted reasoning, a tool
    /// the server runs and its result, a kind it does not know.
    Other,
}

/// A streamed message as its events built it so far.
#[derive(Debug)]
pub(crate) struct Folding<'p> {
    /// The model the request asked for.
    asked: &'p str,
    started: bool,
    stopped: bool,
    /// The open blocks, by their index.
    blocks: BTreeMap<u32, Kind>,
    /// The calls of the message, those whose block is open among them.
    calls: Calls,
    finish_reason: Option<String>,
    /// Why the safeguards declined a refused message.
    refusal: Option<String>,
    /// The model the message's start says answered.
    model: Option<String>,
    usage: Usage,
}

/// The failure of an answer that cannot be folded, in the provider's words.
fn unreadable(what: &'static str) -> Failure {
    Failure::unreadable(what)
}

/// The member `name` of `holder`, an object; a failure in `missing`'s words
/// when it holds none.
fn object<'v>(holder: &'v Value, name: &str, missing: &'static str) -> Result<&'v Value, Failure> {
    let held = holder.get(name).filter(|held| held.is_object());
    held.ok_or_else(|| unreadable(missing))
}

/// The text `delta` holds under `name`.
fn text<'v>(delta: &'v Value, name: &str) -> Result<&'v str, Failure> {
    let held = delta.get(name).and_then(Value::as_str);
    held.ok_or_else(|| unreadable("a delta of a content block holds no text"))
}

/// The index of the content block `event` names: a whole number.
fn index_of(event: &Value) -> Result<u32, Failure> {
    let index = event.get("index").and_then(Value::as_u64);
    let index = index.and_then(|index| u32::try_from(index).ok());
    index.ok_or_else(|| unreadable("a content block names no index"))
}

impl<'p> Folding<'p> {
    /// The folding of a stream asking the model `asked`.
    pub(crate) fn new(asked: &'p str) -> Self {
        Self {
            asked,
            started: false,
            stopped: false,
            blocks: BTreeMap::new(),
            calls: Calls::default(),
            finish_reason: None,
            refusal: None,
            model: None,
            usage: Usage::default(),
        }
    }

    /// Whether the message started: an event of a message came.
    pub(crate) const fn has_started(&self) -> bool {
        self.started
    }

    /// Whether the event that ends the message came.
    pub(crate) const fn has_ended(&self) -> bool {
        self.stopped
    }

    /// Whether the message is complete: it said why it stopped, or the
    /// event that ends it came.
    pub(crate) const fn is_complete(&self) -> bool {
        self.stopped || self.finish_reason.is_some()
    }

    /// Takes the event whose data is `data`, adding to `ready` what it
    /// gives. What it gave before it fails stays in `ready`.
    ///
    /// # Errors
    ///
    /// The failure of an event that is no JSON, of an error the server
    /// wrote in place of an event, and of an event that cannot be folded.
    pub(crate) fn take(&mut self, data: &str, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let event: Value =
            serde_json::from_str(data).map_err(|error| Failure::malformed(&error))?;
        if !event.is_object() {
            return Err(unreadable("an event of the stream is no object"));
        }
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            // An event that names no type and holds an error is the
            // stream's error.
            let error = event.get("error").filter(|error| said_something(error));
            return Err(error.map_or_else(
                || unreadable("an event of the stream names no type"),
                |_| Failure::error_object(&event),
            ));
        };
        match kind {
            "error" => Err(Failure::error_object(&event)),
            "message_start" => self.start(&event, ready),
            "content_block_start" => self.started()?.open(&event, ready),
            "content_block_delta" => self.started()?.delta(&event, ready),
            "content_block_stop" => self.started()?.close(&event, ready),
            "message_delta" => self.started()?.end(&event),
            "message_stop" => {
                self.started()?.stopped = true;
                Ok(())
            }
            // A ping, and an event of a kind the provider does not know.
            _ => Ok(()),
        }
    }

    /// The folding, once its message started.
    fn started(&mut self) -> Result<&mut Self, Failure> {
        if self.started {
            Ok(self)
        } else {
            Err(unreadable("an event of the message came before its start"))
        }
    }

    /// The start of the message: the model that answers, and what it tells
    /// of its input, which a generation cut before its end counts.
    fn start(&mut self, event: &Value, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        if self.started {
            return Err(unreadable("the message starts twice"));
        }
        let message = object(
            event,
            "message",
            "the start of the message holds no message",
        )?;
        self.started = true;
        let model = message.get("model").and_then(Value::as_str);
        self.model = model.filter(|model| !model.is_empty()).map(str::to_owned);
        self.usage.start(message.get("usage"));
        let told = self.usage.of_the_input();
        if !told.is_empty() {
            ready.push(StreamEvent::Usage(StreamUsage { usage: told }));
        }
        Ok(())
    }

    /// A content block opens: a call is told at once, before any of its
    /// arguments, and a redacted block of reasoning gives its opaque data.
    fn open(&mut self, event: &Value, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let index = index_of(event)?;
        let block = object(
            event,
            "content_block",
            "a content block opens with no block",
        )?;
        if self.blocks.contains_key(&index) {
            return Err(unreadable("a content block opens twice"));
        }
        if self.blocks.len() >= OPEN_BLOCKS_LIMIT {
            return Err(unreadable(
                "the answer holds more than 1024 open content blocks",
            ));
        }
        let kind = match block.get("type").and_then(Value::as_str) {
            Some("text") => Kind::Text,
            Some("thinking") => Kind::Thinking,
            Some("tool_use") => {
                ready.push(StreamEvent::ToolCallDelta(self.calls.open(index, block)?));
                Kind::Call
            }
            Some("redacted_thinking") => {
                let data = block.get("data").and_then(Value::as_str);
                let data = data.ok_or_else(|| unreadable("a redacted block holds no data"))?;
                ready.push(StreamEvent::ThinkingDelta(StreamThinkingDelta {
                    block: Some(index),
                    redacted: Some(data.to_owned()),
                    ..StreamThinkingDelta::default()
                }));
                Kind::Other
            }
            Some(_) | None => Kind::Other,
        };
        self.blocks.insert(index, kind);
        Ok(())
    }

    /// A delta of an open block: a piece of text, of reasoning, a block's
    /// signature, or a fragment of a call's arguments. A delta of another
    /// kind says nothing, and neither do the arguments of a tool the server
    /// runs.
    fn delta(&mut self, event: &Value, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let index = index_of(event)?;
        let no_block = || unreadable("a delta names no open content block");
        let block = *self.blocks.get(&index).ok_or_else(no_block)?;
        let delta = object(event, "delta", "a delta of a content block holds no delta")?;
        let thought = |thinking: &str, signature: Option<&str>| {
            StreamEvent::ThinkingDelta(StreamThinkingDelta {
                block: Some(index),
                signature: signature.map(str::to_owned),
                ..StreamThinkingDelta::new(thinking)
            })
        };
        let given = match (delta.get("type").and_then(Value::as_str), block) {
            (Some("text_delta"), Kind::Text) => {
                StreamEvent::TextDelta(StreamTextDelta::new(text(delta, "text")?))
            }
            (Some("thinking_delta"), Kind::Thinking) => thought(text(delta, "thinking")?, None),
            (Some("signature_delta"), Kind::Thinking) => {
                thought("", Some(text(delta, "signature")?))
            }
            (Some("text_delta" | "thinking_delta" | "signature_delta"), _) => {
                return Err(unreadable("a delta does not match its content block"));
            }
            (Some("input_json_delta"), Kind::Call) => {
                let fragment = text(delta, "partial_json")?;
                StreamEvent::ToolCallDelta(self.calls.fragment(index, fragment)?)
            }
            (Some(_) | None, _) => return Ok(()),
        };
        ready.push(given);
        Ok(())
    }

    /// A content block closes: a call's is the call itself. A closed block
    /// was written whole: arguments that do not read make a call that does
    /// not run, never one the model garbled.
    fn close(&mut self, event: &Value, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let index = index_of(event)?;
        let closed = self.blocks.remove(&index);
        let closed = closed.ok_or_else(|| unreadable("a content block closes that is not open"))?;
        let call = (closed == Kind::Call).then(|| self.calls.close(index));
        ready.extend(call.flatten().map(StreamEvent::ToolCall));
        Ok(())
    }

    /// The end of the message: why it stopped, the category of a refusal,
    /// and what it used, which replaces what its start told.
    fn end(&mut self, event: &Value) -> Result<(), Failure> {
        let delta = object(event, "delta", "the end of the message holds no delta")?;
        let said = |held: Option<&Value>| {
            let said = held.and_then(Value::as_str).filter(|said| !said.is_empty());
            said.map(str::to_owned)
        };
        self.finish_reason = said(delta.get("stop_reason"));
        let details = delta.get("stop_details");
        self.refusal = said(details.and_then(|details| details.get("category")));
        self.usage.end(event.get("usage"));
        Ok(())
    }

    /// Adds to `ready` the end of a message that is complete: the calls its
    /// stream left open, each read for how the message stopped, then its
    /// done event, which names the model that answered, else the one asked.
    pub(crate) fn finish(self, ready: &mut Vec<StreamEvent>) {
        let left_open = self.calls.left_open(self.finish_reason.as_deref());
        ready.extend(left_open.map(StreamEvent::ToolCall));
        let mut metadata = Metadata::new();
        let model = self.model.unwrap_or_else(|| self.asked.to_owned());
        metadata.insert("model".to_owned(), model.into());
        if let Some(refusal) = self.refusal {
            metadata.insert("refusal".to_owned(), refusal.into());
        }
        ready.push(StreamEvent::Done(StreamDone {
            finish_reason: self.finish_reason,
            usage: self.usage.whole(),
            metadata,
        }));
    }
}
