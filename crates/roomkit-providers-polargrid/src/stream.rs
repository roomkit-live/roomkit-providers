//! A streamed answer read as it arrives: polargrid-sdk's `_stream_post`, as
//! RoomKit's `sdk_patch` reads it, then Python's `_stream_events`.
//!
//! The SDK reads the answer line by line, not by event: a line that starts
//! with `data:` holds a payload, `[DONE]` ends the stream, a payload that is
//! no JSON is skipped. A payload that names an `error` fails the stream,
//! retried by its words, as a failure the SDK dropped the status of. A
//! chunk the SDK does not read as one is skipped (the edge's closing
//! `pg_metadata` among them); the usage the edge sends last is read apart,
//! and dropped when it lacks one of its three counts. A stream that gave
//! nothing fails (Python's `nonempty_stream`). The stream is tried once,
//! whatever the configuration's `max_retries`: the SDK tries none again.

use roomkit::model::Metadata;
use roomkit_ai::{
    AiContext, ProviderStream, StreamDone, StreamEvent, StreamTextDelta, StreamThinkingDelta,
    StreamToolCallDelta, dumps, read_tool_calls, stream_call_of,
};
use roomkit_providers_client::Call;
use roomkit_providers_client::chat::call_slots::{CallSlots, Fragment, FragmentId};
use roomkit_providers_client::chat::chat_finish_reason;
use roomkit_providers_client::chat::think_tags::{Segment, ThinkTagParser};
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::items::{Yielder, items};
use roomkit_providers_client::stream::Item;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::failure::Failure;
use crate::provider::PolarGrid;

/// The mark that ends a stream.
const END_MARK: &str = "[DONE]";

/// A chunk, as the SDK reads it (`ChatCompletionChunk`).
#[derive(Deserialize)]
struct Chunk {
    #[expect(dead_code, reason = "read for the SDK's check: a chunk names its id")]
    id: String,
    #[serde(default = "chunk_kind")]
    object: String,
    #[expect(dead_code, reason = "read for the SDK's check: a chunk is dated")]
    created: i64,
    model: String,
    choices: Vec<Choice>,
}

fn chunk_kind() -> String {
    "chat.completion.chunk".to_owned()
}

/// A choice of a chunk (`ChatCompletionChunkChoice`).
#[derive(Deserialize)]
struct Choice {
    #[expect(
        dead_code,
        reason = "read for the SDK's check: a choice names its index"
    )]
    index: i64,
    delta: Delta,
    #[serde(default)]
    finish_reason: Option<Finish>,
}

/// The finish reasons the SDK takes.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Finish {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
}

impl Finish {
    const fn word(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Length => "length",
            Self::ContentFilter => "content_filter",
            Self::ToolCalls => "tool_calls",
        }
    }
}

/// What a choice adds (`ChatDelta`).
#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    role: Option<Assistant>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<CallDelta>>,
}

/// The one role a delta names.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Assistant {
    Assistant,
}

/// A fragment of a call (`ToolCallDelta`): its function as it came.
#[derive(Deserialize)]
struct CallDelta {
    index: i64,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<Map<String, Value>>,
}

/// The usage (`TokenUsage`): its three counts.
#[derive(Deserialize)]
#[expect(clippy::struct_field_names, reason = "the wire's names")]
struct Usage {
    prompt_tokens: i64,
    completion_tokens: i64,
    #[expect(
        dead_code,
        reason = "read for the SDK's check: the usage has its total"
    )]
    total_tokens: i64,
}

/// The events of `request`'s answer, its failure as its last item.
pub(crate) fn events<'a>(provider: &'a PolarGrid, request: &'a AiContext) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(provider, request, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

async fn read(
    provider: &PolarGrid,
    request: &AiContext,
    out: &Yielder<Item>,
) -> Result<(), Failure> {
    roomkit_ai::schema_for_generate(request, true, false, crate::provider::LABEL)?;
    let edge = provider.edge().await?;
    let body = provider.asked().body(request, true)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let call = Call {
        url: &edge.chat,
        headers: &provider.streamed_headers,
        body: Some(&body),
        token: None,
    };
    let mut answer = provider.streaming.stream_lines(call).await?;
    let mut folding = Folding::new(&provider.config.model);
    let mut ready = Vec::new();
    while let Some(line) = answer.next().await? {
        let Some(payload) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if payload == END_MARK {
            break;
        }
        let Ok(raw) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        let taken = folding.take(&raw, &mut ready);
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        taken?;
    }
    if !folding.started {
        return Err(Failure::no_event());
    }
    drop(answer);
    folding.finish(&mut ready)?;
    for event in ready {
        out.give(Ok(event)).await;
    }
    Ok(())
}

/// A streamed answer as its chunks built it so far.
struct Folding<'p> {
    calls: CallSlots,
    tags: ThinkTagParser,
    finish_reason: Option<&'static str>,
    usage: Metadata,
    model: Option<String>,
    asked: &'p str,
    /// Whether a chunk or a usage came.
    started: bool,
}

impl<'p> Folding<'p> {
    fn new(asked: &'p str) -> Self {
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

    /// Folds the payload `raw` in: its error, the chunk it is, and the usage
    /// it carries, each as the SDK reads it.
    fn take(&mut self, raw: &Value, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        if let Some(error) = raw.get("error") {
            return Err(Failure::stream_error(error, "stream error"));
        }
        let chunk = Chunk::deserialize(raw)
            .ok()
            .filter(|chunk| chunk.object == "chat.completion.chunk");
        if let Some(chunk) = chunk {
            self.started = true;
            self.chunk(chunk, ready)?;
        }
        let usage = raw
            .get("usage")
            .filter(|usage| roomkit_providers_client::chat::truthy(usage));
        if let Some(usage) = usage.and_then(|usage| Usage::deserialize(usage).ok()) {
            self.started = true;
            let mut read = Metadata::new();
            read.insert("input_tokens".into(), usage.prompt_tokens.into());
            read.insert("output_tokens".into(), usage.completion_tokens.into());
            self.usage = read;
        }
        Ok(())
    }

    fn chunk(&mut self, chunk: Chunk, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        if !chunk.model.is_empty() {
            self.model = Some(chunk.model);
        }
        let Some(choice) = chunk.choices.into_iter().next() else {
            return Ok(());
        };
        let _ = choice.delta.role;
        if let Some(finish) = choice.finish_reason {
            self.finish_reason = Some(finish.word());
        }
        for call in choice.delta.tool_calls.unwrap_or_default() {
            if let Some(composed) = self.fold(&call)? {
                ready.push(StreamEvent::ToolCallDelta(composed));
            }
        }
        if let Some(text) = choice.delta.content.filter(|text| !text.is_empty()) {
            queue(ready, self.tags.feed(&text));
        }
        Ok(())
    }

    /// Folds a call's fragment in; an object for its arguments reads as the
    /// JSON text it stands for.
    fn fold(&mut self, call: &CallDelta) -> Result<Option<StreamToolCallDelta>, Failure> {
        let function = call.function.as_ref();
        let name = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str);
        let arguments = match function.and_then(|function| function.get("arguments")) {
            Some(Value::String(text)) => text.clone(),
            Some(object @ Value::Object(_)) => dumps(object),
            Some(_) | None => String::new(),
        };
        let id = match call.id.as_deref() {
            None | Some("") => FragmentId::None,
            Some(id) => FragmentId::Text(id),
        };
        let fragment = Fragment {
            index: u64::try_from(call.index).unwrap_or(0),
            id,
            name: name.filter(|name| !name.is_empty()),
            arguments: &arguments,
            function: function.is_some(),
        };
        self.calls.fold(&fragment).map_err(Failure::from)
    }

    /// Adds to `ready` what the tags still hold, the calls, each read for
    /// how the answer stopped, then the done event, which names the model
    /// that answered, else the one asked.
    fn finish(mut self, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        queue(ready, self.tags.flush());
        let finish_reason = chat_finish_reason(self.finish_reason);
        let raw = self.calls.finish()?.into_iter();
        let raw = raw.map(|call| (call.id, call.name, call.arguments));
        let calls = read_tool_calls(raw.collect(), finish_reason);
        ready.extend(calls.iter().map(stream_call_of).map(StreamEvent::ToolCall));
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
