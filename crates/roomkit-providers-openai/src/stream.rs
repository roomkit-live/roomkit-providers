//! A streamed answer read as it arrives: the request, the events of its
//! answer through the bounded reader of `roomkit-providers-client`, and
//! each event handed on before the next bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its events are taken, and dropping the stream
//! drops the request with its connection, which closes it.
//!
//! An answer is complete once it said why it stopped, or sent its end
//! mark. One that ends with neither was cut: what came is handed on, then
//! it fails, and a call it brought is never handed whole, to run. An
//! answer that ends inside an event was cut there: nothing of that event
//! is read, and the answer is what it was before it. An answer that gave
//! no event and that the server says is JSON may be an error object in
//! place of a stream: it reads as a whole answer's does.

use roomkit_ai::{AiContext, ProviderStream};
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::items::{Yielder, items};
use roomkit_providers_client::stream::Item;
use serde_json::Value;

use crate::answer::truthy;
use crate::config::OpenAiApi;
use crate::failure::{self, Failure};
use crate::folding::{Folding, push_end};
use crate::provider::OpenAi;
use crate::responses;

/// The mark that ends a stream of chunks.
const END_MARK: &str = "[DONE]";

/// The events of the generation answering `request`, a failure the last
/// of them. The failure has not left the provider yet: it is carried, to
/// be let out once the contract checked the stream.
pub(crate) fn events<'a>(provider: &'a OpenAi, request: &'a AiContext) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        let read = match provider.config.api {
            OpenAiApi::ChatCompletions => read(provider, request, &out).await,
            OpenAiApi::Responses => responses::read(provider, request, &out).await,
        };
        if let Err(failure) = read {
            out.give(Err(carried(failure))).await;
        }
    }))
}

/// The failure of an answer that gave no event: what the error object it
/// holds says, when it is a JSON document that holds one, as the reader of
/// a whole answer reads it, else that no event came.
fn no_event(answer: Option<&Value>) -> Failure {
    let error = answer.and_then(|answer| answer.get("error"));
    match error.filter(|error| truthy(error)) {
        Some(error) => Failure::error_object(error),
        None => Failure::no_event(),
    }
}

/// Asks for the stream and hands its events to `out` as they are read.
///
/// The answer is read as a stream of events whatever the server says it
/// is, as Python's client reads it: a page or a whole JSON body holds no
/// event, which is a failure, the one an error object names when the body
/// is one.
async fn read(provider: &OpenAi, request: &AiContext, out: &Yielder<Item>) -> Result<(), Failure> {
    let body = provider.asked().body(request, true)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let mut answer = provider.client.stream(provider.asking(&body)).await?;
    let mut folding = Folding::new(&provider.config.model);
    let (mut chunks, mut marked, mut ready) = (0_usize, false, Vec::new());
    while let Some(data) = answer.next().await? {
        if data.starts_with(END_MARK) {
            marked = true;
            break;
        }
        // The answer ended inside this event: it is no event of the
        // answer, which is what it was before it.
        if answer.was_cut() {
            break;
        }
        let taken = folding.take(&data, &mut ready);
        // What a chunk gave before it failed is handed on first.
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        taken?;
        chunks = chunks.saturating_add(1);
    }
    // The connection is not read past the end of the answer.
    if chunks == 0 {
        return Err(no_event(answer.unstreamed().as_ref()));
    }
    drop(answer);
    end(folding, marked, out).await
}

/// Hands on the end of an answer that gave events: what its folding still
/// holds, then its calls and its done event when the answer is complete,
/// `marked` saying whether its end mark came.
///
/// # Errors
///
/// The failure of an answer that ended with neither a finish reason nor
/// its end mark, after what came; and the failure of its calls.
async fn end(mut folding: Folding<'_>, marked: bool, out: &Yielder<Item>) -> Result<(), Failure> {
    let mut ready = Vec::new();
    // What the end gave before it failed is handed on first.
    folding.flush(&mut ready);
    let complete = marked || folding.has_finish_reason();
    let finished = if complete {
        let finished = folding.finish();
        finished.map(|(calls, done)| push_end(&mut ready, calls, done))
    } else {
        Err(failure::cut_short())
    };
    for event in ready {
        out.give(Ok(event)).await;
    }
    finished
}

#[cfg(test)]
mod tests;
