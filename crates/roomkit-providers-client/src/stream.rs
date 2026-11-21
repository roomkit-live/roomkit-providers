//! A streamed answer read as it arrives: the events of the answer through
//! the bounded reader of `roomkit-providers-http`, each handed on before
//! the next bytes are read, and how a stream's failure leaves a provider.
//!
//! The stream is pulled: the answer is read only as its events are taken,
//! and dropping it drops the request with its connection, which closes it.

use std::collections::VecDeque;

use roomkit_providers_http::SseError;
use roomkit_providers_http::sse::Events;
/// The most bytes read of a streamed answer, envelopes included.
pub use roomkit_providers_http::sse::STREAM_LIMIT;
use roomkit_providers_llm::{
    AiRequest, ProviderError, ProviderStream, StreamEvent, TextStream, Yielder, items, next,
};
use serde_json::Value;

use crate::client::Client;
use crate::exchange::{ANSWER_LIMIT, Call, is_json};
use crate::failure::{Exit, Failure, past_mark};

/// What a stream of events hands next.
pub type Item = Result<StreamEvent, ProviderError>;

/// A streamed answer whose head came: the data of its events, in order, as
/// its bytes arrive ([`next`](Self::next)).
///
/// The answer is read as a stream of events whatever the server says it
/// is, as Python's client reads it. While it gave no event, the bytes of
/// one the server says is JSON are kept, within what an answer read whole
/// may weigh: such an answer may be a JSON document in place of a stream,
/// which [`unstreamed`](Self::unstreamed) gives.
///
/// An answer may end inside an event, with no blank line after it. Such an
/// event is given as any other, and [`was_cut`](Self::was_cut) says
/// whether the answer was cut there.
pub struct Streamed {
    response: reqwest::Response,
    events: Events,
    /// The events read and not given yet.
    ready: VecDeque<String>,
    /// Why the reading stopped, to say once what was read before is given.
    unread: Option<SseError>,
    /// Whether the answer ended.
    ended: bool,
    /// Whether an event was given.
    given: bool,
    /// The bytes of an answer the server says is JSON, kept until a second
    /// event is asked for.
    kept: Option<Vec<u8>>,
    /// Whether the answer was cut inside the event given last: said at the
    /// answer's end, of its last event, which is given next.
    cut: bool,
}

impl std::fmt::Debug for Streamed {
    /// How far the answer was read: nothing of what it said.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Streamed")
            .field("given", &self.given)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Sends `call` until the head of an answer comes or no try is left:
    /// the answer, to read as a stream of events. A try is taken again
    /// until the head of an answer came, never after: what happens to the
    /// stream afterwards is its reader's.
    ///
    /// # Errors
    ///
    /// The failure that ended the request, as [`send`](Self::send) gives
    /// it, but for an answer's length: a stream is bounded as it is read.
    pub async fn stream(&self, call: Call<'_>) -> Result<Streamed, Failure> {
        let response = self.open(call).await?;
        let kept = is_json(response.headers()).then(Vec::new);
        Ok(Streamed {
            response,
            events: Events::new(self.stream_limit()),
            ready: VecDeque::new(),
            unread: None,
            ended: false,
            given: false,
            kept,
            cut: false,
        })
    }
}

impl Streamed {
    /// The data of the answer's next event, `None` once the answer ended.
    /// An event the answer ends on with no blank line after it is given all
    /// the same, with what came of its data: [`was_cut`](Self::was_cut)
    /// says whether the answer was cut inside it.
    ///
    /// # Errors
    ///
    /// The failure of a connection lost while the answer was read, and, once
    /// every event read before it was given, the failure of an answer past
    /// what the reader reads of one or that is no stream of events.
    pub async fn next(&mut self) -> Result<Option<String>, Failure> {
        if self.given {
            // An answer that gave an event is a stream.
            self.kept = None;
        }
        loop {
            if let Some(data) = self.ready.pop_front() {
                self.given = true;
                return Ok(Some(data));
            }
            // What the reader read before it failed was handed on first.
            if let Some(error) = self.unread.take() {
                self.ended = true;
                return Err(Failure::stream_unread(&error));
            }
            if self.ended {
                return Ok(None);
            }
            let read = self.response.chunk().await;
            let read = read.map_err(|error| Failure::transport(&error))?;
            let (ended, unread) = match &read {
                Some(bytes) => {
                    self.keep(bytes);
                    self.events.feed(bytes)
                }
                None => match self.events.finish() {
                    Ok(last) => {
                        self.cut = last.as_deref().is_some_and(stops_short);
                        (last.into_iter().collect(), None)
                    }
                    Err(error) => (Vec::new(), Some(error)),
                },
            };
            self.ready.extend(ended);
            self.unread = unread;
            self.ended = read.is_none();
        }
    }

    /// Whether the answer was cut inside the event [`next`](Self::next) gave
    /// last: the answer ended with no blank line after that event, and its
    /// data does not read as a JSON document. Such an event is no event of
    /// the answer, which is then what it was before it: complete if it had
    /// said how it ends, cut if it had not, and nothing follows it.
    ///
    /// An event whose data reads as a JSON document came whole: it is read
    /// as any other, with no blank line after it and with no line end
    /// after its last line. The line end cannot tell the two apart: the
    /// last line of a whole event may lack its own, and data written on
    /// several lines may be cut after one. The data can, where an event's
    /// data is a JSON object, as on the wires read here: no start of an
    /// object's text is a JSON document. A wire that ends on a mark that
    /// is no JSON looks for its mark first.
    #[must_use]
    pub const fn was_cut(&self) -> bool {
        self.cut
    }

    /// Keeps `bytes`, the next ones of an answer the server says is JSON.
    fn keep(&mut self, bytes: &[u8]) {
        let Some(kept) = &mut self.kept else {
            return;
        };
        if kept.len().saturating_add(bytes.len()) > ANSWER_LIMIT {
            self.kept = None;
        } else {
            kept.extend_from_slice(bytes);
        }
    }

    /// The JSON document the answer was in place of a stream: for an answer
    /// that gave no event its reader counts, and that the server said is
    /// JSON. `None` for any other body, and for one past what an answer
    /// read whole may weigh. The connection is dropped: nothing more is
    /// read of the answer.
    #[must_use]
    pub fn unstreamed(self) -> Option<Value> {
        let body = self.kept?;
        serde_json::from_slice(past_mark(&body)).ok()
    }
}

/// Whether `data`, what came of an event no blank line followed, stops
/// short of the event's end: it does not read as a JSON document.
fn stops_short(data: &str) -> bool {
    serde_json::from_str::<Value>(data).is_err()
}

/// The events of the generation that answers `request`, as they leave a
/// provider: each failure, the one the stream carried
/// ([`carried`](crate::failure::carried)) or one the contract's check built
/// on the way, through `exit` with `request`, whose own key no failure
/// repeats either ([`Exit::leave_for`]); and nothing after it.
pub fn leaving<'a>(
    mut events: ProviderStream<'a>,
    exit: &'a Exit,
    request: &'a AiRequest,
) -> ProviderStream<'a> {
    Box::pin(items(move |out: Yielder<Item>| async move {
        while let Some(event) = next(&mut events).await {
            match event {
                Ok(event) => out.give(Ok(event)).await,
                Err(error) => {
                    let left = exit.leave_carried(request, error);
                    return out.give(Err(left)).await;
                }
            }
        }
    }))
}

/// The text of `events`, as it comes: its reasoning, its calls and its end
/// are left out, a failure is the last item, as it is. The events are
/// those of a generation that already left ([`leaving`]).
pub fn text(mut events: ProviderStream<'_>) -> TextStream<'_> {
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

#[cfg(test)]
mod tests;
