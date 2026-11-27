//! A streamed answer folded as it arrives: Python's
//! `MistralAIProvider._events`, each chunk read as the mistralai SDK reads
//! it.
//!
//! The SDK reads each chunk into its models before the provider sees it,
//! and a chunk they do not read fails the stream: one without its `id`, its
//! `model` or its `choices`, a choice without its `index`, its `delta` or
//! its `finish_reason` (`null` taken), a call without its function's
//! `name` and `arguments`, a content that is no text and no list, a part of
//! one that is no object or names no `type`. A part the SDK does not know,
//! or does not read as its type, is left out: only a text part and a part
//! of reasoning hold anything the provider reads.

use roomkit::model::Metadata;
use roomkit_ai::{
    FinishReason, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta, dumps,
    read_tool_calls, stream_call_of,
};
use roomkit_providers_client::chat::call_slots::{CallSlots, Fragment, FragmentId};
use roomkit_providers_client::chat::chat_finish_reason;
use roomkit_providers_client::chat::think_tags::{Segment, ThinkTagParser};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::failure::{self, Failure};

/// A chunk as the SDK reads it (`CompletionChunk`).
#[derive(Deserialize)]
struct Chunk {
    #[expect(dead_code, reason = "read for the SDK's check: a chunk names its id")]
    id: String,
    model: String,
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

/// A choice of a chunk (`CompletionResponseStreamChoice`).
#[derive(Deserialize)]
struct Choice {
    #[expect(
        dead_code,
        reason = "read for the SDK's check: a choice names its index"
    )]
    index: i64,
    delta: Delta,
    #[serde(deserialize_with = "required")]
    finish_reason: Option<String>,
}

/// What a choice adds (`DeltaMessage`).
#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<Content>,
    #[serde(default)]
    tool_calls: Option<Vec<Call>>,
}

/// A delta's content: a text, or typed parts.
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<Part>),
}

/// A part of a content, as the SDK reads it: an object that names its type,
/// whatever it holds.
struct Part(Value);

impl<'de> Deserialize<'de> for Part {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.get("type").is_none() {
            return Err(serde::de::Error::custom("a content part names no type"));
        }
        Ok(Self(value))
    }
}

/// A part of reasoning (`ThinkChunk`), as the SDK reads one: each of its
/// pieces a text or a reference.
#[derive(Deserialize)]
struct Thinking {
    thinking: Vec<Piece>,
}

/// A piece of a part of reasoning.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Piece {
    Text {
        text: String,
    },
    Reference {
        #[expect(
            dead_code,
            reason = "read for the SDK's check: a reference names its ids"
        )]
        reference_ids: Vec<i64>,
    },
}

/// A text part (`TextChunk`).
#[derive(Deserialize)]
struct Text {
    text: String,
}

/// A call, or a fragment of one (`ToolCall`).
#[derive(Deserialize)]
struct Call {
    function: Function,
    #[serde(default)]
    id: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    index: u64,
}

/// A call's function (`FunctionCall`): its arguments a text or an object.
#[derive(Deserialize)]
struct Function {
    name: String,
    arguments: Arguments,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Arguments {
    Text(String),
    Object(serde_json::Map<String, Value>),
}

/// The usage a chunk reports (`UsageInfo`): a count that is `null` reads as
/// none.
#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<Value>,
}

/// A member that must be there, `null` taken.
fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// A member that reads `null` as absent: its type's default.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The id of a call as Python reads it: none for an absent one, and for
/// `"null"`, which the SDK fills a missing id with (Python's
/// `_server_call_id`).
fn server_call_id(id: Option<&str>) -> FragmentId<'_> {
    match id {
        None | Some("" | "null") => FragmentId::None,
        Some(id) => FragmentId::Text(id),
    }
}

/// Mistral's stop reasons in the contract's vocabulary: the dialect's, and
/// the context window filling up mid-answer (Python's
/// `_MISTRAL_FINISH_REASONS`).
fn finish_reason_of(word: Option<&str>) -> Option<FinishReason> {
    match word {
        Some("model_length") => Some(FinishReason::Length),
        other => chat_finish_reason(other),
    }
}

/// The usage a chunk reports, cached prompt tokens apart from fresh input
/// (Python's `_usage_from`).
fn usage_of(usage: &Usage) -> Metadata {
    let prompt = usage.prompt_tokens.unwrap_or(0);
    let details = usage.prompt_tokens_details.as_ref();
    let cached = details.and_then(|details| details.get("cached_tokens"));
    let cached = cached.and_then(Value::as_u64).unwrap_or(0);
    let mut read = Metadata::new();
    read.insert("input_tokens".into(), prompt.saturating_sub(cached).into());
    let output = usage.completion_tokens.unwrap_or(0);
    read.insert("output_tokens".into(), output.into());
    if cached != 0 {
        read.insert("cache_read_input_tokens".into(), cached.into());
    }
    read
}

/// A streamed answer as its chunks built it so far.
pub(crate) struct Folding<'p> {
    calls: CallSlots,
    tags: ThinkTagParser,
    finish_reason: Option<String>,
    usage: Metadata,
    /// The model the last chunk naming one says answered.
    model: Option<String>,
    /// The model the request asked for.
    asked: &'p str,
    /// Whether a chunk came.
    started: bool,
}

impl<'p> Folding<'p> {
    pub(crate) fn new(asked: &'p str) -> Self {
        Self {
            calls: CallSlots::default(),
            tags: ThinkTagParser::default(),
            finish_reason: None,
            usage: Metadata::new(),
            model: None,
            asked,
            started: false,
        }
    }

    /// Whether a chunk came.
    pub(crate) const fn has_started(&self) -> bool {
        self.started
    }

    /// Folds the chunk `data` in, adding to `ready` what it settles: the
    /// composition of its calls, then its reasoning and its text.
    ///
    /// # Errors
    ///
    /// A chunk the SDK does not read, and a call that does not fold.
    pub(crate) fn take(&mut self, data: &str, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let chunk: Chunk = serde_json::from_str(data).map_err(|error| failure::no_chunk(&error))?;
        self.started = true;
        if !chunk.model.is_empty() {
            self.model = Some(chunk.model);
        }
        // The usage is read before the choices: a server may report it on
        // a chunk of its own, with none.
        if let Some(usage) = &chunk.usage {
            self.usage = usage_of(usage);
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(());
        };
        if let Some(reason) = choice.finish_reason.filter(|reason| !reason.is_empty()) {
            self.finish_reason = Some(reason);
        }
        for call in choice.delta.tool_calls.unwrap_or_default() {
            if let Some(composed) = self.fold(&call)? {
                ready.push(StreamEvent::ToolCallDelta(composed));
            }
        }
        match choice.delta.content {
            Some(Content::Text(text)) if !text.is_empty() => queue(ready, self.tags.feed(&text)),
            Some(Content::Parts(parts)) => queue(ready, segments(&parts)),
            Some(Content::Text(_)) | None => {}
        }
        Ok(())
    }

    /// Adds a call's fragment to the call it belongs to; an object for its
    /// arguments reads as the JSON text it stands for.
    fn fold(&mut self, call: &Call) -> Result<Option<roomkit_ai::StreamToolCallDelta>, Failure> {
        let arguments = match &call.function.arguments {
            Arguments::Text(text) => text.clone(),
            Arguments::Object(object) => dumps(&Value::Object(object.clone())),
        };
        let name = call.function.name.as_str();
        let fragment = Fragment {
            index: call.index,
            id: server_call_id(call.id.as_deref()),
            name: Some(name).filter(|name| !name.is_empty()),
            arguments: &arguments,
            function: true,
        };
        self.calls.fold(&fragment).map_err(Failure::from)
    }

    /// Adds to `ready` the end of the answer: what the tags still hold, its
    /// calls, each read for how the answer stopped, then its done event,
    /// which names the model that answered, else the one asked.
    ///
    /// # Errors
    ///
    /// The failure of a call that does not read.
    pub(crate) fn finish(mut self, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        queue(ready, self.tags.flush());
        let finish_reason = finish_reason_of(self.finish_reason.as_deref());
        let raw = self.calls.finish()?.into_iter();
        let raw = raw.map(|call| (call.id, call.name, call.arguments));
        let calls = read_tool_calls(raw.collect(), finish_reason);
        let calls = calls.iter().map(stream_call_of);
        ready.extend(calls.map(StreamEvent::ToolCall));
        let mut done = StreamDone {
            finish_reason,
            usage: self.usage,
            metadata: Metadata::new(),
        };
        let model = self.model.unwrap_or_else(|| self.asked.to_owned());
        done.metadata.insert("model".to_owned(), model.into());
        ready.push(StreamEvent::Done(done));
        Ok(())
    }
}

/// The reasoning and the text of typed parts, in order (Python's
/// `_chunks_to_segments`): a part the SDK does not read as its type, or of
/// another type, is left out.
fn segments(parts: &[Part]) -> Vec<Segment> {
    let mut read = Vec::new();
    for Part(part) in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("thinking") => {
                let Ok(thinking) = Thinking::deserialize(part) else {
                    continue;
                };
                for piece in thinking.thinking {
                    if let Piece::Text { text } = piece
                        && !text.is_empty()
                    {
                        read.push(Segment::Thinking(text));
                    }
                }
            }
            Some("text") => {
                if let Ok(Text { text }) = Text::deserialize(part)
                    && !text.is_empty()
                {
                    read.push(Segment::Text(text));
                }
            }
            _ => {}
        }
    }
    read
}

/// `segments` as events, in order.
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
