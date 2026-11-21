//! The `OpenAI` provider as Python holds it; what it sends and reads stays
//! in Rust's `OpenAiAiProvider`.
//!
//! JSON text crosses, both ways, in the contract's forms: a request as one
//! text, a whole answer as one text, and a stream, at each crossing, as the
//! list of every item already received.
//!
//! A stream is read on the bridge between two crossings, a bounded number
//! of items ahead of what Python took: so a crossing finds what came
//! meanwhile, and a burst of events costs a few crossings, not one each.

use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use futures_core::Stream;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use roomkit_providers_core::{AiProvider, AiRequest, ProviderError, Yielder, items, next};
use roomkit_providers_openai::{OpenAiAiProvider, OpenAiConfig, ReplayWaits};
use serde::Serialize;
use tokio::sync::Mutex as TaskMutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_util::sync::CancellationToken;

use super::errors::Failure;
use super::{fork, runtime};

/// The most items one crossing of a stream brings back, and the most that
/// are read ahead of what Python took. A crossing takes what is there and
/// never waits for more.
const BATCH_LIMIT: usize = 64;

/// `value` as JSON text.
fn written<T: Serialize>(value: &T, what: &str) -> Result<String, Failure> {
    serde_json::to_string(value).map_err(|_| {
        let said = format!("{what} could not be written as JSON");
        Failure::Provider(ProviderError::from(said))
    })
}

/// The request `json` writes, in the contract's form.
fn request_of(json: &str) -> Result<AiRequest, Failure> {
    serde_json::from_str(json).map_err(|error| Failure::Value(error.to_string()))
}

/// The configuration `json` writes, in the form of Python's `OpenAIConfig`.
fn config_of(json: &str) -> PyResult<OpenAiConfig> {
    serde_json::from_str(json).map_err(|error| PyValueError::new_err(error.to_string()))
}

/// The JSON form of the configuration `config_json` writes, read and
/// written back: what the reader refuses is a `ValueError`, a member left
/// out takes its default, and the two switches a modern model decides on
/// `OpenAI`'s own endpoint are written decided. The key is written masked.
#[pyfunction]
pub(crate) fn openai_config_form(config_json: &str) -> PyResult<String> {
    let config = config_of(config_json)?;
    written(&config, "the configuration").map_err(Failure::raised)
}

#[pyclass(name = "_OpenAIProvider", frozen, module = "roomkit_providers._native")]
#[derive(Debug)]
pub(crate) struct PythonOpenAiProvider {
    provider: OpenAiAiProvider,
    /// The waits of a provider a replay drives, which takes none.
    waits: Option<ReplayWaits>,
}

#[pymethods]
impl PythonOpenAiProvider {
    /// The provider of the configuration `config_json` writes, with `api_key`
    /// as its key: the key crosses apart, never inside the JSON text.
    #[new]
    fn new(config_json: &str, api_key: &str) -> PyResult<Self> {
        let mut config = config_of(config_json)?;
        api_key.clone_into(&mut config.api_key);
        let provider =
            OpenAiAiProvider::new(config).map_err(|error| Failure::from(error).raised())?;
        Ok(Self {
            provider,
            waits: None,
        })
    }

    #[getter]
    fn model_name(&self) -> &str {
        self.provider.model_name()
    }

    #[getter]
    fn supports_vision(&self) -> bool {
        self.provider.supports_vision()
    }

    #[getter]
    fn supports_streaming(&self) -> bool {
        self.provider.supports_streaming()
    }

    #[getter]
    fn supports_structured_streaming(&self) -> bool {
        self.provider.supports_structured_streaming()
    }

    #[getter]
    fn supports_response_schema(&self) -> bool {
        self.provider.supports_response_schema()
    }

    #[getter]
    fn supports_response_schema_with_tools(&self) -> bool {
        self.provider.supports_response_schema_with_tools()
    }

    #[getter]
    fn supports_deferred_tools(&self) -> bool {
        self.provider.supports_deferred_tools()
    }

    #[getter]
    fn context_window(&self) -> Option<u64> {
        self.provider.context_window()
    }

    /// The catalogue's entry for the configured model as JSON, or None.
    fn catalog_entry_json(&self) -> PyResult<Option<String>> {
        let entry = self.provider.catalog_entry();
        let entry = entry.map(|entry| written(&entry, "the catalogue's entry"));
        entry.transpose().map_err(Failure::raised)
    }

    /// The curated catalogue as a JSON list, with no request.
    #[staticmethod]
    fn available_models_json() -> PyResult<String> {
        let models = OpenAiAiProvider::available_models();
        written(&models, "the catalogue").map_err(Failure::raised)
    }

    /// The models the endpoint lists, as a JSON list.
    fn list_models<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.provider.clone();
        runtime::call(py, async move {
            let listed = provider.list_models().await.map_err(Failure::from);
            let listed = listed.and_then(|models| written(&models, "the model listing"));
            listed.map_err(Failure::raised)
        })
    }

    /// One whole answer to the request `request_json` writes, as JSON.
    fn generate<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.provider.clone();
        runtime::call(py, async move {
            let answered = async {
                let request = request_of(&request_json)?;
                let response = provider.generate(&request).await?;
                written(&response, "the response")
            };
            answered.await.map_err(Failure::raised)
        })
    }

    /// The stream of one generation answering the request `request_json`
    /// writes: its events, or its text alone when `text` is set. Nothing
    /// is read or sent before the stream's first `next()`.
    fn stream(&self, request_json: String, text: bool) -> PyResult<PythonStream> {
        fork::ensure_unforked()?;
        let feed = feed(self.provider.clone(), request_json, text);
        let (sender, receiver) = mpsc::channel(BATCH_LIMIT);
        Ok(PythonStream {
            unbegun: Mutex::new(Some((feed, sender))),
            inbox: Arc::new(TaskMutex::new(Inbox {
                receiver,
                failed: None,
            })),
            closed: CancellationToken::new(),
        })
    }

    /// Releases what the provider holds, as Rust's `close()`.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = self.provider.clone();
        runtime::call(py, async move {
            provider.close().await;
            Ok(())
        })
    }

    /// For the replay of the recorded cases, never for a host: the same
    /// provider sending to `host:port`, a loopback address, over plain
    /// HTTP, reading a `retry-after` date against `clock` and keeping its
    /// waits instead of taking them.
    fn _replaying(&self, host: &str, port: u16, clock: f64) -> PyResult<Self> {
        let host: IpAddr = host
            .parse()
            .map_err(|_| PyValueError::new_err("a replay's fake is named by its IP address"))?;
        let replaying = self
            .provider
            .clone()
            .replaying(SocketAddr::new(host, port), clock);
        let (provider, waits) = replaying.map_err(|error| Failure::from(error).raised())?;
        Ok(Self {
            provider,
            waits: Some(waits),
        })
    }

    /// The waits, in seconds, a replayed provider did not take between its
    /// tries.
    fn _waits(&self) -> Vec<f64> {
        self.waits
            .as_ref()
            .map(ReplayWaits::taken)
            .unwrap_or_default()
    }
}

/// An item of one generation as JSON text, or the failure that ends it.
type Item = Result<String, Failure>;

/// The items of one generation, then its failure.
type Feed = Pin<Box<dyn Stream<Item = Item> + Send>>;

/// The stream of `provider` answering the request `request_json` writes,
/// owning both: each event as JSON, or each piece of text as a JSON string
/// when `text` is set. A request that does not read is its one failure.
fn feed(provider: OpenAiAiProvider, request_json: String, text: bool) -> Feed {
    Box::pin(items(move |out: Yielder<Item>| async move {
        let request = match request_of(&request_json) {
            Ok(request) => request,
            Err(refused) => return out.give(Err(refused)).await,
        };
        if text {
            let mut pieces = provider.generate_stream(&request);
            while let Some(piece) = next(&mut pieces).await {
                let piece = piece.map_err(Failure::from);
                out.give(piece.and_then(|piece| written(&piece, "the text")))
                    .await;
            }
        } else {
            let mut events = provider.generate_structured_stream(&request);
            while let Some(event) = next(&mut events).await {
                let event = event.map_err(Failure::from);
                out.give(event.and_then(|event| written(&event, "the event")))
                    .await;
            }
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
