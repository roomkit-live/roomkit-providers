//! A streamed answer read as it arrives: the request to Mistral's chat
//! completions, its events through the bounded reader of
//! `roomkit-providers-client`, and each event handed on before the next
//! bytes are read.
//!
//! The stream is pulled: nothing is sent before it is first polled, the
//! answer is read only as its events are taken, and dropping the stream
//! drops the request with its connection, which closes it. As the mistralai
//! SDK reads it, a 2xx answer is read only when it says it is a stream of
//! events; the stream ends at its `[DONE]` mark, or with the answer, whose
//! last event is read even with no blank line after it; and a stream that
//! carried no chunk fails (Python's `nonempty_stream`). Nothing more is
//! asked of how the answer ends: one without a finish reason is complete.

use roomkit_ai::{AiContext, ProviderStream};
use roomkit_providers_client::Call;
use roomkit_providers_client::failure::carried;
use roomkit_providers_client::items::{Yielder, items};
use roomkit_providers_client::stream::Item;

use crate::failure::{self, Failure};
use crate::folding::Folding;
use crate::provider::Mistral;

/// The mark that ends a stream of chunks.
const END_MARK: &str = "[DONE]";

/// The events of `request`'s answer, its failure as its last item.
pub(crate) fn events<'a>(provider: &'a Mistral, request: &'a AiContext) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        if let Err(failure) = read(provider, request, &out).await {
            out.give(Err(carried(failure))).await;
        }
    }))
}

async fn read(provider: &Mistral, request: &AiContext, out: &Yielder<Item>) -> Result<(), Failure> {
    let body = provider.asked().body(request)?;
    let body = serde_json::to_vec(&body).map_err(|_| Failure::unwritten())?;
    let call = Call {
        url: &provider.chat,
        headers: &provider.chat_headers,
        body: Some(&body),
        token: None,
    };
    let mut answer = provider.client.stream(call).await?;
    if !answer.is_event_stream() {
        return Err(failure::no_stream());
    }
    let mut folding = Folding::new(&provider.config.model);
    let mut ready = Vec::new();
    while let Some(data) = answer.next().await? {
        if data == END_MARK {
            break;
        }
        let taken = folding.take(&data, &mut ready);
        // What a chunk gave before it failed is handed on first.
        for event in ready.drain(..) {
            out.give(Ok(event)).await;
        }
        taken?;
    }
    if !folding.has_started() {
        return Err(Failure::no_event());
    }
    drop(answer);
    let finished = folding.finish(&mut ready);
    for event in ready {
        out.give(Ok(event)).await;
    }
    finished
}
