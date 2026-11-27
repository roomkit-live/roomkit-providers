//! A streamed answer read as it arrives: the request to `/api/chat`, its
//! JSON lines through the bounded reader of `roomkit-providers-client`, and
//! each event handed on before the next bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its lines are taken, and dropping the stream
//! drops the request with its connection, which closes it. As the ollama
//! SDK reads it, each line is a JSON object, a blank one included, and one
//! that names an `error` fails the stream, retried as a lost connection; a
//! stream that carried no line fails (Python's `nonempty_stream`). The
//! calls come whole, each on a line: they are handed on once the stream is
//! over, after its text, then its end.

use roomkit::model::Metadata;
use roomkit_ai::{
    AiContext, AiToolCall, CallIds, FinishReason, ProviderStream, StreamDone, StreamEvent,
    StreamTextDelta, StreamThinkingDelta, stream_call_of,
};
use roomkit_providers_client::Call;
use roomkit_providers_client::chat::truthy;
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::items::{Yielder, items};
use roomkit_providers_client::stream::Item;
use serde::Deserialize;
use serde_json::Value;

use crate::failure::{self, Failure};
use crate::provider::Ollama;
use crate::reading::Answer;

/// The events of `request`'s answer, its failure as its last item.
pub(crate) fn events<'a>(provider: &'a Ollama, request: &'a AiContext) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(provider, request, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

async fn read(provider: &Ollama, request: &AiContext, out: &Yielder<Item>) -> Result<(), Failure> {
    let body = provider.asked().body(request, true).await?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let call = Call {
        url: &provider.chat,
        headers: &provider.headers,
        body: Some(&body),
        token: None,
    };
    let mut answer = provider.client.stream_lines(call).await?;
    let mut folding = Folding::new(&provider.config.model);
    let mut ready = Vec::new();
    while let Some(line) = answer.next().await? {
        let taken = folding.take(&line, &mut ready);
        // What a line gave before it failed is handed on first.
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        taken?;
    }
    if !folding.started {
        return Err(Failure::no_event());
    }
    drop(answer);
    folding.finish(&mut ready);
    for event in ready {
        out.give(Ok(event)).await;
    }
    Ok(())
}

/// A streamed answer as its lines built it so far.
struct Folding<'p> {
    calls: Vec<AiToolCall>,
    ids: CallIds,
    finish_reason: Option<FinishReason>,
    usage: Metadata,
    /// The model the last line naming one says answered.
    model: Option<String>,
    /// The model the request asked for.
    asked: &'p str,
    /// Whether a line came.
    started: bool,
}

impl<'p> Folding<'p> {
    fn new(asked: &'p str) -> Self {
        Self {
            calls: Vec::new(),
            ids: CallIds::new(),
            finish_reason: None,
            usage: Metadata::new(),
            model: None,
            asked,
            started: false,
        }
    }

    /// Folds the line `text` in, adding to `ready` its reasoning, then its
    /// text.
    fn take(&mut self, text: &str, ready: &mut Vec<StreamEvent>) -> Result<(), Failure> {
        let line: Value = serde_json::from_str(text).map_err(|error| failure::unread(&error))?;
        let Some(members) = line.as_object() else {
            return Err(failure::no_object());
        };
        if let Some(error) = members.get("error").filter(|error| truthy(error)) {
            return Err(failure::aborted(error));
        }
        let answer = Answer::deserialize(&line).map_err(|error| failure::unread(&error))?;
        self.started = true;
        if let Some(model) = answer.answered_by() {
            self.model = Some(model.to_owned());
        }
        let message = &answer.message;
        let thinking = message.thinking.as_deref().filter(|text| !text.is_empty());
        if let Some(thinking) = thinking {
            ready.push(StreamEvent::ThinkingDelta(StreamThinkingDelta::new(
                thinking,
            )));
        }
        let content = message.content.as_deref().filter(|text| !text.is_empty());
        if let Some(content) = content {
            ready.push(StreamEvent::TextDelta(StreamTextDelta::new(content)));
        }
        self.calls.extend(message.calls(&mut self.ids));
        if answer.done == Some(true) {
            self.finish_reason = answer.finish_reason();
            self.usage = answer.usage();
        }
        Ok(())
    }

    /// Adds to `ready` the end of the answer: its calls, then its done
    /// event, which names the model that answered, else the one asked.
    fn finish(self, ready: &mut Vec<StreamEvent>) {
        let calls = self.calls.iter().map(stream_call_of);
        ready.extend(calls.map(StreamEvent::ToolCall));
        let mut done = StreamDone {
            finish_reason: self.finish_reason,
            usage: self.usage,
            metadata: Metadata::new(),
        };
        let model = self.model.unwrap_or_else(|| self.asked.to_owned());
        done.metadata.insert("model".to_owned(), model.into());
        ready.push(StreamEvent::Done(done));
    }
}
