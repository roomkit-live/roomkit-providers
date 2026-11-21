//! A streamed completion folded as its chunks come: what Python's
//! `_stream_events` reads of each chunk, in its order.
//!
//! A chunk gives the model that answers, the usage, and, of its first
//! choice, why the answer stopped, a piece of a refusal, the fragments of
//! its calls, a piece of reasoning and a piece of text. A member a server
//! wrote in another shape than the dialect's reads as absent, as in a
//! whole answer; what Python fails a stream on fails it here: a chunk that
//! is no object, choices that are no list, a call that is no call, a text
//! or a finish reason that is no text.

use roomkit_providers_core::{
    Metadata, ProviderError, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta,
    StreamToolCallDelta, dumps, read_tool_calls, stream_call_of,
};
use serde::Deserialize;
use serde_json::Value;

use crate::answer::{truthy, usage_of};
use crate::call_slots::{CallSlots, Fragment, FragmentId, RawCall};
use crate::failure::Failing;
use crate::think_tags::{Segment, ThinkTagParser, field_reasoning};

/// A fragment of a call, which the call's `index` names across chunks.
#[derive(Debug, Deserialize)]
struct DeltaCall {
    #[serde(default, deserialize_with = "nullable")]
    index: u64,
    /// The server's id, as it wrote it: [`fragment_id`] reads it.
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    function: Option<DeltaFunction>,
}

#[derive(Debug, Deserialize)]
struct DeltaFunction {
    #[serde(default)]
    name: Option<String>,
    /// A fragment of the arguments' JSON text; some servers send an object.
    #[serde(default)]
    arguments: Value,
}

/// A member that reads `null` as absent: its type's default.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// A stream's call id as Python's `ToolCallSlots` reads what the server
/// wrote: none for `null` or empty text, the text, and an id that is no
/// text kept as it reads, false or true, for the fold to judge.
fn fragment_id(id: Option<&Value>) -> FragmentId<'_> {
    match id {
        None | Some(Value::Null) => FragmentId::None,
        Some(Value::String(id)) if id.is_empty() => FragmentId::None,
        Some(Value::String(id)) => FragmentId::Text(id),
        Some(id) if truthy(id) => FragmentId::True(id),
        Some(_) => FragmentId::False,
    }
}

/// A streamed completion as its chunks built it so far.
#[derive(Debug)]
pub(crate) struct Folding<'p> {
    calls: CallSlots,
    finish_reason: Option<String>,
    usage: Metadata,
    /// Sorts the text into reasoning and answer, a tag split across chunks
    /// included.
    tags: ThinkTagParser,
    /// The refusal the deltas carried, in their order.
    refusal: String,
    /// The model the last chunk naming one says answered.
    model: Option<Value>,
    /// The model the request asked for.
    asked: &'p str,
    failing: Failing<'p>,
}

impl<'p> Folding<'p> {
    /// The folding of a stream asking the model `asked`.
    pub(crate) fn new(asked: &'p str, failing: Failing<'p>) -> Self {
        Self {
            calls: CallSlots::default(),
            finish_reason: None,
            usage: Metadata::new(),
            tags: ThinkTagParser::default(),
            refusal: String::new(),
            model: None,
            asked,
            failing,
        }
    }

    /// Takes the chunk `data`, adding to `ready` what it gives: the
    /// composition of its calls, its reasoning and its text, in order.
    /// What it gave before it fails stays in `ready`.
    ///
    /// # Errors
    ///
    /// The failure of an event that is no JSON, of an error the server
    /// wrote in place of a chunk, and of a chunk that does not read.
    pub(crate) fn take(
        &mut self,
        data: &str,
        ready: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        let failing = self.failing;
        let chunk: Value = serde_json::from_str(data).map_err(|error| failing.malformed(&error))?;
        if let Some(error) = chunk.get("error").filter(|error| truthy(error)) {
            return Err(failing.stream_error(error));
        }
        if !chunk.is_object() {
            return Err(self.unread("an event of the stream is no chat completion chunk"));
        }
        if let Some(model) = chunk.get("model").filter(|model| truthy(model)) {
            self.model = Some(model.clone());
        }
        let usage = chunk.get("usage");
        if let Some(usage) = usage.filter(|usage| usage.is_object() || truthy(usage)) {
            self.usage = usage_of(usage);
        }
        let first = match chunk.get("choices") {
            Some(Value::Array(choices)) => choices.first(),
            Some(other) if truthy(other) => {
                return Err(self.unread("the choices of a chunk are no list"));
            }
            Some(_) | None => None,
        };
        match first {
            Some(choice) if choice.is_object() => self.read_choice(choice, ready),
            Some(_) => Err(self.unread("a choice of a chunk is no object")),
            None => Ok(()),
        }
    }

    /// The failure of a chunk that does not read: final.
    fn unread(&self, what: &str) -> ProviderError {
        self.failing.unreadable(what)
    }

    /// Reads the first choice of a chunk: Python reads no other.
    fn read_choice(
        &mut self,
        choice: &Value,
        ready: &mut Vec<StreamEvent>,
    ) -> Result<(), ProviderError> {
        // The last reason that says something is kept, as Python's `or`
        // keeps it.
        match choice.get("finish_reason").filter(|reason| truthy(reason)) {
            Some(Value::String(reason)) => self.finish_reason = Some(reason.clone()),
            Some(_) => return Err(self.unread("the finish reason of a chunk is no text")),
            None => {}
        }
        let Some(delta) = choice.get("delta").filter(|delta| delta.is_object()) else {
            return Ok(());
        };
        if let Some(refusal) = delta.get("refusal").and_then(Value::as_str) {
            self.refusal.push_str(refusal);
        }
        match delta.get("tool_calls") {
            Some(Value::Array(calls)) => {
                for call in calls {
                    if let Some(composition) = self.fold(call)? {
                        ready.push(StreamEvent::ToolCallDelta(composition));
                    }
                }
            }
            Some(other) if truthy(other) => {
                return Err(self.unread("the tool calls of a chunk are no list"));
            }
            Some(_) | None => {}
        }
        let reasoning = field_reasoning(delta.get("reasoning_content"), delta.get("reasoning"));
        if let Some(reasoning) = reasoning {
            queue(ready, vec![Segment::Thinking(reasoning)]);
        }
        // The text is checked only here, after the chunk's calls and its
        // reasoning, which Python hands on before it reads the text.
        match delta.get("content").filter(|content| truthy(content)) {
            Some(Value::String(text)) => queue(ready, self.tags.feed(text)),
            Some(_) => return Err(self.unread("the text of a chunk is no text")),
            None => {}
        }
        Ok(())
    }

    /// Adds a call's fragment to the call it belongs to; an object for its
    /// arguments reads as the JSON text it stands for.
    fn fold(&mut self, call: &Value) -> Result<Option<StreamToolCallDelta>, ProviderError> {
        let failing = self.failing;
        let call = DeltaCall::deserialize(call).map_err(|error| failing.malformed(&error))?;
        let function = call.function.as_ref();
        let name = function.and_then(|function| function.name.as_deref());
        let arguments = match function.map(|function| &function.arguments) {
            Some(Value::String(text)) => text.clone(),
            Some(object @ Value::Object(_)) => dumps(object),
            Some(other) if truthy(other) => {
                return Err(self.unread("the arguments of a call are no text and no object"));
            }
            Some(_) | None => String::new(),
        };
        let fragment = Fragment {
            index: call.index,
            id: fragment_id(call.id.as_ref()),
            name: name.filter(|name| !name.is_empty()),
            arguments: &arguments,
            function: function.is_some(),
        };
        let folded = self.calls.fold(&fragment);
        folded.map_err(|reason| self.unread(&reason))
    }

    /// Adds to `ready` what the tags still hold once the stream is over.
    pub(crate) fn flush(&mut self, ready: &mut Vec<StreamEvent>) {
        queue(ready, self.tags.flush());
    }

    /// The completion once its stream is over: its calls in the order they
    /// came, each with the text of its arguments, and how it ended. Its end
    /// names the model that answered, else the one asked, and keeps a
    /// refusal.
    ///
    /// # Errors
    ///
    /// The failure of a call that took an id that is no text.
    pub(crate) fn finish(self) -> Result<(Vec<RawCall>, StreamDone), ProviderError> {
        let failing = self.failing;
        let calls = self.calls.finish();
        let calls = calls.map_err(|reason| failing.unreadable(&reason))?;
        let mut done = StreamDone {
            finish_reason: self.finish_reason,
            usage: self.usage,
            metadata: Metadata::new(),
        };
        let model = self.model.unwrap_or_else(|| self.asked.into());
        done.metadata.insert("model".to_owned(), model);
        if !self.refusal.is_empty() {
            let refusal = self.refusal.into();
            done.metadata.insert("refusal".to_owned(), refusal);
        }
        Ok((calls, done))
    }
}

/// Adds the end of an answer to `ready`: its calls, each read for how the
/// answer stopped, then its done event.
pub(crate) fn push_end(ready: &mut Vec<StreamEvent>, calls: Vec<RawCall>, done: StreamDone) {
    let raw = calls.into_iter();
    let raw = raw.map(|call| (call.id, call.name, call.arguments));
    let calls = read_tool_calls(raw.collect(), done.finish_reason.as_deref());
    let calls = calls.iter().map(stream_call_of);
    ready.extend(calls.map(StreamEvent::ToolCall));
    ready.push(StreamEvent::Done(done));
}

/// `segments` as events, in the tag parser's order.
fn queue(ready: &mut Vec<StreamEvent>, segments: Vec<Segment>) {
    for segment in segments {
        ready.push(match segment {
            Segment::Thinking(thinking) => {
                StreamEvent::ThinkingDelta(StreamThinkingDelta::new(thinking))
            }
            Segment::Text(text) => StreamEvent::TextDelta(StreamTextDelta::new(text)),
        });
    }
}

#[cfg(test)]
mod tests;
