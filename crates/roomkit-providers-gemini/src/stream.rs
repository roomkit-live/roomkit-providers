//! A streamed answer read as it arrives: the request to
//! `streamGenerateContent?alt=sse`, its chunks through the bounded reader
//! of `roomkit-providers-client`, and each event handed on before the next
//! bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its chunks are taken, and dropping the stream
//! drops the request with its connection, which closes it. A stream that
//! fails hands on first what it gave; the round's calls and the end come
//! only after its last chunk, as Python's provider hands them. A stream
//! that carried no chunk fails: its body could not be read, or it is an
//! error object in place of a stream.

use roomkit_ai::{AiContext, ProviderStream};
use roomkit_providers_client::Call;
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::items::{Yielder, items};
use roomkit_providers_client::stream::Item;
use serde_json::Value;

use crate::failure::{self, Failure};
use crate::folding::Folding;
use crate::provider::Gemini;

/// The events of `request`'s answer, its failure as its last item.
pub(crate) fn events<'a>(provider: &'a Gemini, request: &'a AiContext) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(provider, request, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

async fn read(provider: &Gemini, request: &AiContext, out: &Yielder<Item>) -> Result<(), Failure> {
    let body = provider.asked().body(request)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let call = Call {
        url: &provider.generate,
        headers: &provider.headers,
        body: Some(&body),
        token: provider.token_source.as_deref(),
    };
    let mut answer = provider.client.stream(call).await?;
    let mut folding = Folding::new(&provider.config.model);
    let mut ready = Vec::new();
    while let Some(data) = answer.next().await? {
        if answer.was_cut() {
            break;
        }
        let chunk: Value = match serde_json::from_str(&data) {
            Ok(chunk) => chunk,
            Err(error) => return Err(Failure::malformed(&error)),
        };
        folding.take(&chunk, &mut ready);
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
    }
    if !folding.has_started() {
        return Err(failure::no_event(answer.unstreamed().as_ref()));
    }
    drop(answer);
    folding.finish(&mut ready);
    for event in ready {
        out.give(Ok(event)).await;
    }
    Ok(())
}
