//! A streamed answer read as it arrives: the request, the events of its
//! answer through the bounded reader of `roomkit-providers-http`, and each
//! event handed on before the next bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its events are taken, and dropping the stream
//! drops the request with its connection, which closes it.

use roomkit_providers_core::{
    AiRequest, ProviderError, ProviderStream, StreamEvent, TextStream, Yielder, items, next,
};
use roomkit_providers_http::sse::Events;

use crate::exchange::Call;
use crate::folding::{Folding, push_end};
use crate::provider::{OpenAiAiProvider, unwritten};

/// What a stream of events hands next.
type Item = Result<StreamEvent, ProviderError>;

/// The mark that ends a stream of chunks.
const END_MARK: &str = "[DONE]";

/// The events of the generation answering `request`, a failure the last
/// of them.
pub(crate) fn events<'a>(
    provider: &'a OpenAiAiProvider,
    request: &'a AiRequest,
) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(error) = read(provider, request, &out).await {
            out.give(Err(error)).await;
        }
    }))
}

/// The text of `events`, as it comes: its reasoning, its calls and its end
/// are left out, a failure is the last item.
pub(crate) fn text(mut events: ProviderStream<'_>) -> TextStream<'_> {
    Box::pin(items(
        move |out: Yielder<Result<String, ProviderError>>| async move {
            while let Some(event) = next(&mut events).await {
                match event {
                    Ok(StreamEvent::TextDelta(delta)) => out.give(Ok(delta.text)).await,
                    Ok(
                        StreamEvent::ToolCall(_)
                        | StreamEvent::ToolCallDelta(_)
                        | StreamEvent::Done(_)
                        | StreamEvent::ThinkingDelta(_)
                        | StreamEvent::Usage(_),
                    ) => {}
                    Err(error) => return out.give(Err(error)).await,
                }
            }
        },
    ))
}

/// Asks for the stream and hands its events to `out` as they are read.
///
/// The answer is read as a stream of events whatever the server says it
/// is, as Python's client reads it: a page or a whole JSON body holds no
/// event, which is a failure.
async fn read(
    provider: &OpenAiAiProvider,
    request: &AiRequest,
    out: &Yielder<Item>,
) -> Result<(), ProviderError> {
    let body = provider.asked().body(request, true)?;
    let body = serde_json::to_vec(&body).map_err(|_| unwritten())?;
    let mut response = provider.open(Call::Chat(&body)).await?;
    let failing = provider.failing();
    let mut events = Events::new(provider.stream_limit);
    let mut folding = Folding::new(&provider.config.model, failing);
    let (mut chunks, mut ready) = (0_usize, Vec::new());
    'answer: loop {
        let read = response.chunk().await;
        let read = read.map_err(|error| failing.transport(&error))?;
        let ended = match &read {
            Some(bytes) => events.feed(bytes),
            // An event the answer ends on with no blank line after it is
            // read all the same.
            None => events.finish().map(|last| last.into_iter().collect()),
        };
        for data in ended.map_err(|error| failing.stream_unread(&error))? {
            if data.starts_with(END_MARK) {
                break 'answer;
            }
            let taken = folding.take(&data, &mut ready);
            // What a chunk gave before it failed is handed on first.
            for event in ready.drain(..) {
                out.give(Ok(event)).await;
            }
            taken?;
            chunks = chunks.saturating_add(1);
        }
        if read.is_none() {
            break;
        }
    }
    // The connection is not read past the end of the answer.
    drop(response);
    if chunks == 0 {
        return Err(failing.no_event());
    }
    // What the end gave before it failed is handed on first.
    folding.flush(&mut ready);
    let finished = folding.finish();
    let finished = finished.map(|(calls, done)| push_end(&mut ready, calls, done));
    for event in ready {
        out.give(Ok(event)).await;
    }
    finished
}

#[cfg(test)]
mod tests;
