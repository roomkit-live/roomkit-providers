//! A streamed answer read as it arrives: the request, the events of its
//! answer through the bounded reader of `roomkit-providers-client`, and
//! each event handed on before the next bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its events are taken, and dropping the stream
//! drops the request with its connection, which closes it.
//!
//! An answer is complete once it said why it stopped, or sent the event
//! that ends its message, past which nothing is read. One that ends with
//! neither was cut: what came is handed on, a call whose block closed
//! among it, then it fails. A call still open is never handed, and the
//! failure is the stream's last item: a host runs no call of a stream that
//! failed. An answer that ends inside an event was cut there: nothing of
//! that event is read, and the answer is what it was before it. An answer
//! that gave no event of a message and that the server says is JSON may be
//! an error object in place of a stream: it fails as the status that
//! object names.

use roomkit_providers_client::Call;
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::stream::Item;
use roomkit_providers_llm::{AiRequest, ProviderStream, Yielder, items};

use crate::failure::{self, Failure};
use crate::folding::Folding;
use crate::provider::Anthropic;

/// The events of the generation answering `request`, a failure the last
/// of them. The failure has not left the provider yet: it is carried, to
/// be let out once the contract checked the stream.
pub(crate) fn events<'a>(provider: &'a Anthropic, request: &'a AiRequest) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(provider, request, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

/// Asks for the stream and hands its events to `out` as they are read.
///
/// The answer is read as a stream of events whatever the server says it
/// is, as Python's client reads it: a page or a whole JSON body holds no
/// event, which is a failure, the one an error object names when the body
/// is one.
async fn read(
    provider: &Anthropic,
    request: &AiRequest,
    out: &Yielder<Item>,
) -> Result<(), Failure> {
    let body = provider.asked().body(request)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let headers = provider.headers_of(request)?;
    let call = Call {
        url: &provider.messages,
        headers: &headers,
        body: Some(&body),
    };
    let mut answer = provider.client.stream(call).await?;
    let mut folding = Folding::new(&provider.config.model);
    let mut ready = Vec::new();
    while let Some(data) = answer.next().await? {
        // The answer ended inside this event: it is no event of the
        // message, which is what it was before it.
        if answer.was_cut() {
            break;
        }
        let taken = folding.take(&data, &mut ready);
        // What an event gave before it failed is handed on first.
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        taken?;
        // The connection is not read past the end of the message.
        if folding.has_ended() {
            break;
        }
    }
    if !folding.has_started() {
        return Err(failure::no_event(answer.unstreamed().as_ref()));
    }
    drop(answer);
    if !folding.is_complete() {
        return Err(failure::cut_short());
    }
    folding.finish(&mut ready);
    for event in ready {
        out.give(Ok(event)).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
