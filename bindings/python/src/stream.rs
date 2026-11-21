//! One generation's stream as Python reads it.
//!
//! A stream is read on the bridge between two crossings, a bounded number
//! of items ahead of what Python took: so a crossing finds what came
//! meanwhile, and a burst of events costs a few crossings, not one each.
//! Each crossing hands over every item already received, as one JSON list.

use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use futures_core::Stream;
use pyo3::prelude::*;
use roomkit_providers_llm::{Yielder, items, next};
use tokio::sync::Mutex as TaskMutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_util::sync::CancellationToken;

use super::errors::Failure;
use super::provider::{Held, request_of, written};
use super::{fork, runtime};

/// The most items one crossing of a stream brings back, and the most that
/// are read ahead of what Python took. A crossing takes what is there and
/// never waits for more.
const BATCH_LIMIT: usize = 64;

/// An item of one generation as JSON text, or the failure that ends it.
type Item = Result<String, Failure>;

/// The items of one generation, then its failure.
type Feed = Pin<Box<dyn Stream<Item = Item> + Send>>;

/// The stream of `provider` answering the request `request_json` writes,
/// owning both: each event as JSON. `api_key` is the credential of this one
/// request. A request that does not read is its one failure.
fn feed(provider: Held, request_json: String, api_key: Option<String>) -> Feed {
    Box::pin(items(move |out: Yielder<Item>| async move {
        let request = match request_of(&request_json, api_key) {
            Ok(request) => request,
            Err(refused) => return out.give(Err(refused)).await,
        };
        let mut events = provider.dyn_generate_structured_stream(&request);
        while let Some(event) = next(&mut events).await {
            let event = event.map_err(Failure::from);
            out.give(event.and_then(|event| written(&event, "the event")))
                .await;
        }
    }))
}

/// Reads `feed` to its end, each item sent to the stream's inbox, which
/// holds [`BATCH_LIMIT`] of them: the reading waits there while Python
/// takes none. It stops when the stream is `closed`, or once nothing
/// receives. The feed is dropped with it, and its connection closed.
async fn read_ahead(mut feed: Feed, inbox: Sender<Item>, closed: CancellationToken) {
    let reading = async {
        while let Some(item) = next(&mut feed).await {
            if inbox.send(item).await.is_err() {
                return;
            }
        }
    };
    // Closed or read to its end, the reading is over either way.
    let _: Option<()> = closed.run_until_cancelled(reading).await;
}

/// What was read of a stream and not yet handed to Python.
struct Inbox {
    receiver: Receiver<Item>,
    /// The failure that came after items already handed on: the next
    /// crossing raises it.
    failed: Option<Failure>,
}

impl Inbox {
    /// One crossing of a stream: the items already received, as a JSON
    /// list, `None` once the stream ended. It waits for the first item and
    /// for no other. A failure comes after the items read before it, at
    /// the next crossing. Dropped while it waits, it loses nothing.
    async fn crossing(&mut self) -> Result<Option<String>, Failure> {
        if let Some(failure) = self.failed.take() {
            return Err(failure);
        }
        let Some(mut item) = self.receiver.recv().await else {
            return Ok(None);
        };
        let mut batch = String::from("[");
        let mut held = 0_usize;
        loop {
            match item {
                Ok(json) => {
                    if held != 0 {
                        batch.push(',');
                    }
                    batch.push_str(&json);
                    held = held.saturating_add(1);
                }
                Err(failure) if held == 0 => return Err(failure),
                Err(failure) => {
                    self.failed = Some(failure);
                    break;
                }
            }
            if held >= BATCH_LIMIT {
                break;
            }
            // What is there now, and nothing that is still to come.
            match self.receiver.try_recv() {
                Ok(more) => item = more,
                Err(_) => break,
            }
        }
        batch.push(']');
        Ok(Some(batch))
    }
}

#[pyclass(name = "_Stream", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonStream {
    /// What the first crossing begins to read: nothing is sent before.
    unbegun: Mutex<Option<(Feed, Sender<Item>)>>,
    inbox: Arc<TaskMutex<Inbox>>,
    /// Cancelled when Python closes the stream, or no longer holds it.
    closed: CancellationToken,
}

impl std::fmt::Debug for PythonStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PythonStream")
            .field("closed", &self.closed.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl Drop for PythonStream {
    /// A stream Python no longer holds is closed: its reading stops.
    fn drop(&mut self) {
        self.closed.cancel();
    }
}

impl PythonStream {
    /// The stream of `provider` answering the request `request_json`
    /// writes, unbegun.
    pub(crate) fn of(
        provider: Held,
        request_json: String,
        api_key: Option<String>,
    ) -> PyResult<Self> {
        fork::ensure_unforked()?;
        let feed = feed(provider, request_json, api_key);
        let (sender, receiver) = mpsc::channel(BATCH_LIMIT);
        Ok(Self {
            unbegun: Mutex::new(Some((feed, sender))),
            inbox: Arc::new(TaskMutex::new(Inbox {
                receiver,
                failed: None,
            })),
            closed: CancellationToken::new(),
        })
    }
}

#[pymethods]
impl PythonStream {
    /// The items already received, as a JSON list, or None once the stream
    /// ended or was closed. The first call begins the reading.
    fn next<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let unbegun = self
            .unbegun
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((feed, inbox)) = unbegun {
            runtime::spawn(read_ahead(feed, inbox, self.closed.clone()))?;
        }
        let inbox = Arc::clone(&self.inbox);
        let closed = self.closed.clone();
        runtime::call(py, async move {
            let mut inbox = inbox.lock().await;
            match closed.run_until_cancelled(inbox.crossing()).await {
                Some(crossed) => crossed.map_err(Failure::raised),
                None => Ok(None),
            }
        })
    }

    /// Private diagnostic: how many items were read ahead and wait for the
    /// next crossing, or None while a crossing is under way.
    fn _ahead(&self) -> Option<usize> {
        let inbox = self.inbox.try_lock().ok()?;
        Some(inbox.receiver.len())
    }

    /// Ends the stream: its reading stops, which drops its request and
    /// closes its connection, and what was read ahead is given up.
    fn close(&self) {
        self.closed.cancel();
        drop(
            self.unbegun
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }
}
