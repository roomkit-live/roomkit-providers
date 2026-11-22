//! Stateless Responses, with provider-owned output replay inside one tool
//! loop.
//!
//! A request is always streamed, and a whole answer is its stream read to
//! its end. Text arrives as it is written; the calls wait for the terminal
//! event, which alone releases them.

mod folding;
mod items;
mod replay;
mod transcript;
mod wire;

use roomkit_ai::{AiContext, AiResponse, ProviderError, ProviderStream, StreamEvent};
use roomkit_providers_client::items::{Yielder, next};
use roomkit_providers_client::stream::Item;

use self::folding::{Folding, push_end, push_text, push_thinking};
pub(crate) use self::wire::{input, request};
use crate::failure::Failure;
use crate::provider::OpenAi;

/// Asks for the answer and hands its events to `out` as they are read,
/// until its terminal event.
///
/// # Errors
///
/// What is refused before any request; the failure of the request; an
/// event that is not the Responses wire's; and an answer that ended before
/// its terminal event, after what came.
pub(crate) async fn read(
    provider: &OpenAi,
    request: &AiContext,
    out: &Yielder<Item>,
) -> Result<(), Failure> {
    let body = provider.asked().responses_body(request)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let mut answer = provider.client.stream(provider.asking(&body)).await?;
    let namespace = provider.config.tool_namespace.as_ref();
    let namespace = namespace.map(|(name, _)| name.as_str());
    let mut folding = Folding::default();
    folding.namespace = namespace.map(str::to_owned);
    let (mut events, mut ready) = (0_usize, Vec::new());
    while let Some(data) = answer.next().await? {
        // The answer ended inside this event: it is no event of the
        // answer, which is what it was before it.
        if answer.was_cut() {
            break;
        }
        let ended = folding.take(&data, &mut ready);
        // What an event gave before it failed is handed on first.
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        if ended? {
            return Ok(());
        }
        events = events.saturating_add(1);
    }
    // An answer that gave no event and that the server says is JSON is a
    // response in place of its stream: it reads as a terminal event's does.
    let whole = if events == 0 {
        answer.unstreamed()
    } else {
        None
    };
    let Some(whole) = whole else {
        let reason = "Responses stream ended before its terminal event";
        return Err(wire::malformed(reason));
    };
    let whole = wire::body(&whole, namespace)?;
    push_thinking(&mut ready, whole.thinking.unwrap_or_default());
    push_text(&mut ready, whole.text);
    push_end(&mut ready, whole.calls, whole.done);
    for event in ready {
        out.give(Ok(event)).await;
    }
    Ok(())
}

/// The whole answer `events` give: their text, their reasoning, their
/// calls, how it ended; `None` for events that end with no done event.
///
/// # Errors
///
/// The failure that ended the events.
pub(crate) async fn whole(
    mut events: ProviderStream<'_>,
) -> Result<Option<AiResponse>, ProviderError> {
    let mut response = AiResponse::new("");
    while let Some(event) = next(&mut events).await {
        match event? {
            StreamEvent::ToolCallDelta(_) | StreamEvent::Usage(_) => {}
            StreamEvent::TextDelta(delta) => response.content.push_str(&delta.text),
            StreamEvent::ThinkingDelta(delta) => {
                let thinking = response.thinking.get_or_insert_default();
                thinking.push_str(&delta.thinking);
            }
            StreamEvent::ToolCall(call) => response.tool_calls.push(call.into()),
            StreamEvent::Done(done) => {
                response.finish_reason = done.finish_reason;
                response.usage = done.usage;
                response.metadata = done.metadata;
                return Ok(Some(response));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
