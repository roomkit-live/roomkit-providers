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

use super::errors::{Failure, guarded};
use super::fork::{self, Kept};
use super::provider::{Held, request_of, written};
use super::runtime;

/// The most items one crossing of a stream brings back, and the most that
/// are read ahead of what Python took. A crossing takes what is there and
/// never waits for more.
const BATCH_LIMIT: usize = 64;

/// What a stream's reading sends its inbox.
enum Item {
    /// An event of the generation, as JSON text.
    Event(String),
    /// The failure that ends the generation.
    Failed(Failure),
    /// The generation read to its end.
    End,
}

/// The events of one generation as JSON text, then its failure.
type Feed = Pin<Box<dyn Stream<Item = Result<String, Failure>> + Send>>;

/// The stream of `provider` answering the request `request_json` writes,
/// owning both: each event as JSON. `api_key` is the credential of this one
/// request, which no failure of the stream repeats. A request that does not
/// read is its one failure.
fn feed(provider: Held, request_json: String, api_key: Option<String>) -> Feed {
    Box::pin(items(
        move |out: Yielder<Result<String, Failure>>| async move {
            let lent = api_key.clone();
            let request = match request_of(&request_json, api_key, provider.dyn_label()) {
                Ok(request) => request,
                Err(refused) => return out.give(Err(refused.without(lent.as_deref()))).await,
            };
            let mut events = provider.dyn_generate_structured_stream(&request);
            while let Some(event) = next(&mut events).await {
                let event = event.map_err(Failure::from);
                let event = event.and_then(|event| written(&event, "the event"));
                out.give(event.map_err(|failure| failure.without(lent.as_deref())))
                    .await;
            }
        },
    ))
}

/// Reads `feed` to its end, each item sent to the stream's inbox, which
/// holds [`BATCH_LIMIT`] of them: the reading waits there while Python
/// takes none. The inbox is told how the stream ended: read to its end, on
/// a failure, or on a panic, which is one. The reading stops when the
/// stream is `closed`, or once nothing receives. The feed is dropped with
/// it, and its connection closed.
async fn read_ahead(mut feed: Feed, inbox: Sender<Item>, closed: CancellationToken, label: String) {
    let reading = async {
        loop {
            let item = match runtime::caught(next(&mut feed)).await {
                Some(Some(Ok(event))) => Item::Event(event),
                Some(Some(Err(failure))) => Item::Failed(failure),
                Some(None) => Item::End,
                None => Item::Failed(Failure::panicked(&label)),
            };
            let last = !matches!(item, Item::Event(_));
            if inbox.send(item).await.is_err() || last {
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
    /// Whether the stream said how it ended.
    ended: bool,
    /// The label of the provider that is read.
    label: String,
}

impl Inbox {
    /// One crossing of a stream: the items already received, as a JSON
    /// list, `None` once the stream ended. It waits for the first item and
    /// for no other. A failure comes after the items read before it, at
    /// the next crossing. A stream whose reading stopped without saying
    /// how it ended is one that failed, unless it was `closed`: closing a
    /// stream stops its reading, which this crossing may see before it sees
    /// the stream closed. Dropped while it waits, it loses nothing.
    async fn crossing(&mut self, closed: &CancellationToken) -> Result<Option<String>, Failure> {
        if let Some(failure) = self.failed.take() {
            return Err(failure);
        }
        if self.ended {
            return Ok(None);
        }
        let Some(mut item) = self.receiver.recv().await else {
            if closed.is_cancelled() {
                return Ok(None);
            }
            self.ended = true;
            return Err(Failure::unended(&self.label));
        };
        let mut batch = String::from("[");
        let mut held = 0_usize;
        loop {
            match item {
                Item::Event(json) => {
                    if held != 0 {
                        batch.push(',');
                    }
                    batch.push_str(&json);
                    held = held.saturating_add(1);
                }
                Item::End | Item::Failed(_) if held == 0 => {
                    self.ended = true;
                    return match item {
                        Item::Failed(failure) => Err(failure),
                        Item::End | Item::Event(_) => Ok(None),
                    };
                }
                Item::End => {
                    self.ended = true;
                    break;
                }
                Item::Failed(failure) => {
                    self.ended = true;
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

/// What a stream holds of the bridge's.
struct Reading {
    /// What the first crossing begins to read: nothing is sent before.
    unbegun: Mutex<Option<(Feed, Sender<Item>)>>,
    inbox: Arc<TaskMutex<Inbox>>,
    /// Cancelled when Python closes the stream, or no longer holds it.
    closed: CancellationToken,
    label: String,
}

#[pyclass(name = "_Stream", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonStream {
    reading: Kept<Reading>,
}

impl std::fmt::Debug for PythonStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let closed = self
            .reading
            .get()
            .map(|reading| reading.closed.is_cancelled());
        formatter
            .debug_struct("PythonStream")
            .field("closed", &closed)
            .finish_non_exhaustive()
    }
}

impl Drop for PythonStream {
    /// A stream Python no longer holds is closed: its reading stops. A
    /// forked child stops nothing: the reading is its parent's.
    fn drop(&mut self) {
        if !fork::is_forked()
            && let Some(reading) = self.reading.get()
        {
            reading.closed.cancel();
        }
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
        let label = provider.dyn_label().to_owned();
        let feed = feed(provider, request_json, api_key);
        let (sender, receiver) = mpsc::channel(BATCH_LIMIT);
        let inbox = Inbox {
            receiver,
            failed: None,
            ended: false,
            label: label.clone(),
        };
        Ok(Self {
            reading: Kept::new(Reading {
                unbegun: Mutex::new(Some((feed, sender))),
                inbox: Arc::new(TaskMutex::new(inbox)),
                closed: CancellationToken::new(),
                label,
            }),
        })
    }
}

#[pymethods]
impl PythonStream {
    /// The items already received, as a JSON list, or None once the stream
    /// ended or was closed. The first call begins the reading.
    fn next<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let Some(reading) = self.reading.get() else {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "the stream was released",
            ));
        };
        guarded(py, &reading.label, || Self::crossing(py, reading))
    }

    /// Private diagnostic: how many items were read ahead and wait for the
    /// next crossing, the stream's end among them, or None while a crossing
    /// is under way.
    fn _ahead(&self) -> Option<usize> {
        let inbox = self.reading.get()?.inbox.try_lock().ok()?;
        Some(inbox.receiver.len())
    }

    /// Ends the stream: its reading stops, which drops its request and
    /// closes its connection, and what was read ahead is given up.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let Some(reading) = self.reading.get() else {
            return Ok(());
        };
        guarded(py, &reading.label, || {
            reading.closed.cancel();
            drop(
                reading
                    .unbegun
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take(),
            );
            Ok(())
        })
    }
}

impl PythonStream {
    /// The awaitable of one crossing of `reading`, whose first begins the
    /// reading.
    fn crossing<'py>(py: Python<'py>, reading: &Reading) -> PyResult<Bound<'py, PyAny>> {
        let unbegun = reading
            .unbegun
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some((feed, inbox)) = unbegun {
            let (closed, label) = (reading.closed.clone(), reading.label.clone());
            runtime::spawn(read_ahead(feed, inbox, closed, label))?;
        }
        let inbox = Arc::clone(&reading.inbox);
        let closed = reading.closed.clone();
        runtime::call(py, &reading.label, async move {
            let mut inbox = inbox.lock().await;
            match closed.run_until_cancelled(inbox.crossing(&closed)).await {
                Some(crossed) => crossed,
                None => Ok(None),
            }
        })
    }
}
