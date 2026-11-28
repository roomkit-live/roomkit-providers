//! Any image provider of the contract (RFC §25) as Python holds it: what it
//! sends and reads stays in its own crate.
//!
//! A generation is a call Python reads as a stream of items, in JSON: each
//! attempt the provider tells its progress callback, then the outcome, the
//! images or the failure. Python tells the attempt to its host's callback
//! and answers the call (`reply`), which the provider's callback waits for:
//! a host's callback that fails fails the generation, as it does in Python,
//! and no thread of the bridge ever runs Python. Closing the call drops the
//! generation, whose attempts in flight are then told unknown, through the
//! same call, which Python reads to its end.

use std::collections::{BTreeMap, HashMap};
use std::error::Error as _;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use roomkit_ai::image::{
    GenerateOptions, GenerateWithOptionsOptions, ImageAttempt, ImageCatalogEntry, ImageError,
    ImageGenerationError, ImageOptions, ImageProgressCallback, ImageProvider, ImageResult,
};
use roomkit_ai::{AiImagePart, ProviderError};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex as TaskMutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::errors::{Failure, guarded};
use super::fork::{self, Kept};
use super::provider::written;
use super::runtime;

/// What an image provider's call gives once awaited, held as an object.
type Held<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An [`ImageProvider`] as an object, which every image provider is.
pub(crate) trait AnyImageProvider: Send + Sync + 'static {
    fn dyn_name(&self) -> &str;
    fn dyn_model_name(&self) -> &str;
    fn dyn_supports_editing(&self) -> bool;
    fn dyn_catalog_entry(&self) -> Option<ImageCatalogEntry>;
    fn dyn_generate<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateOptions,
    ) -> Held<'a, Result<Vec<ImageResult>, ImageError>>;
    fn dyn_generate_with_options<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateWithOptionsOptions,
    ) -> Held<'a, Result<Vec<ImageResult>, ImageError>>;
    fn dyn_close(&self) -> Held<'_, ()>;
}

impl<P: ImageProvider> AnyImageProvider for P {
    fn dyn_name(&self) -> &str {
        self.name()
    }

    fn dyn_model_name(&self) -> &str {
        self.model_name()
    }

    fn dyn_supports_editing(&self) -> bool {
        self.supports_editing()
    }

    fn dyn_catalog_entry(&self) -> Option<ImageCatalogEntry> {
        self.catalog_entry()
    }

    fn dyn_generate<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateOptions,
    ) -> Held<'a, Result<Vec<ImageResult>, ImageError>> {
        Box::pin(self.generate(prompt, options))
    }

    fn dyn_generate_with_options<'a>(
        &'a self,
        prompt: &'a str,
        options: GenerateWithOptionsOptions,
    ) -> Held<'a, Result<Vec<ImageResult>, ImageError>> {
        Box::pin(self.generate_with_options(prompt, options))
    }

    fn dyn_close(&self) -> Held<'_, ()> {
        Box::pin(self.close())
    }
}

/// An image provider, whichever its type.
pub(crate) type HeldImages = Arc<dyn AnyImageProvider>;

/// A reference image, as the package writes RoomKit's `AIImagePart`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Part {
    url: String,
    mime_type: Option<String>,
}

impl From<Part> for AiImagePart {
    fn from(part: Part) -> Self {
        Self {
            url: part.url,
            mime_type: part.mime_type,
        }
    }
}

/// What a generation asks, as the package writes it: which entry of the
/// provider it calls, and that entry's arguments. `told` says whether the
/// host gave a progress callback.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asked {
    call: Entry,
    prompt: String,
    size: Option<String>,
    n: u32,
    reference_images: Vec<Part>,
    options: Option<ImageOptions>,
    mask: Option<Part>,
    told: bool,
}

/// The entry of the provider a generation calls.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Entry {
    Generate,
    GenerateWithOptions,
}

/// Where the host's answer to an attempt told goes: none for a callback
/// that returned, the words of its failure for one that raised.
type Answer = oneshot::Sender<Result<(), String>>;

/// What a call sends Python.
enum Item {
    /// An attempt told, as JSON, and where the host's answer goes.
    Told(String, Answer),
    /// How the generation ended, as JSON.
    Outcome(String),
}

/// The callback a generation tells its attempts to: each crosses to
/// Python, and the callback waits for the host's answer.
fn crossing(items: Sender<Item>) -> ImageProgressCallback {
    Arc::new(move |attempt: ImageAttempt| {
        let items = items.clone();
        Box::pin(async move {
            let told = serde_json::to_string(&attempt)
                .map_err(|_| "the attempt could not be written as JSON")?;
            let (answer, answered) = oneshot::channel();
            let sent = items.send(Item::Told(told, answer)).await;
            sent.map_err(|_| "the caller no longer reads the generation")?;
            match answered.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(said)) => Err(said.into()),
                Err(_) => Err("the caller gave no answer".into()),
            }
        })
    })
}

/// `failed` as the item Python raises: a refusal before any request, a
/// failed generation with its attempts, or another failure of the
/// provider.
fn failure_item(failed: &ImageError) -> Value {
    let error = match failed {
        ImageError::Value(refused) => return json!({ "refused": refused.to_string() }),
        ImageError::Provider(error) => error,
    };
    let generation = error
        .source()
        .and_then(|source| source.downcast_ref::<ImageGenerationError>());
    if let Some(generation) = generation {
        return json!({ "failed": {
            "message": generation.message,
            "provider": generation.provider,
            "attempts": generation.attempts,
        } });
    }
    provider_item(error)
}

/// The item of a failure of the provider that is no failed generation.
fn provider_item(error: &ProviderError) -> Value {
    json!({ "error": {
        "message": error.message,
        "retryable": error.retryable,
        "provider": error.provider,
        "status_code": error.status_code,
        "context_overflow": error.context_overflow,
        "before_request": error.before_request,
    } })
}

/// How a generation ended, as the item Python reads.
fn outcome_item(outcome: Option<Result<Vec<ImageResult>, ImageError>>, label: &str) -> String {
    let item = match outcome {
        Some(Ok(images)) => json!({ "images": images }),
        Some(Err(failed)) => failure_item(&failed),
        None => {
            let said = "the provider stopped on an internal error";
            let options = roomkit_ai::ProviderErrorOptions {
                provider: label.to_owned(),
                ..roomkit_ai::ProviderErrorOptions::default()
            };
            provider_item(&ProviderError::new(said, options))
        }
    };
    item.to_string()
}

/// The generation `asked` asks of `provider`, its outcome sent to `items`.
async fn generated(provider: HeldImages, asked: Asked, items: Sender<Item>, label: String) {
    let Asked {
        call,
        prompt,
        size,
        n,
        reference_images,
        options,
        mask,
        told,
    } = asked;
    let reference_images = reference_images.into_iter().map(Into::into).collect();
    let outcome = runtime::caught(async {
        match call {
            Entry::Generate => {
                let options = GenerateOptions {
                    size,
                    n,
                    reference_images,
                };
                provider.dyn_generate(&prompt, options).await
            }
            Entry::GenerateWithOptions => {
                let options = GenerateWithOptionsOptions {
                    size,
                    n,
                    reference_images,
                    options,
                    mask: mask.map(Into::into),
                    on_progress: told.then(|| crossing(items.clone())),
                };
                provider.dyn_generate_with_options(&prompt, options).await
            }
        }
    })
    .await;
    // Python no longer reading, the outcome has no one to go to.
    drop(
        items
            .send(Item::Outcome(outcome_item(outcome, &label)))
            .await,
    );
}

/// What a call sent Python and Python has not done with: each item from
/// the moment it left the generation until Python says it took it, so that
/// an item a cancelled crossing carried is given again; each attempt told
/// until the host answered it.
#[derive(Default)]
struct Ledger {
    /// The items Python has not taken, as JSON, by their token, in order.
    untaken: BTreeMap<u64, String>,
    /// Where each attempt told waits for the host's answer, by its token.
    answers: HashMap<u64, Answer>,
    /// The token the next item is given.
    next: u64,
}

impl Ledger {
    /// Files `item`, which left the generation, under a token of its own.
    fn file(&mut self, item: Item) {
        let token = self.next;
        self.next = self.next.saturating_add(1);
        let written = match item {
            Item::Told(told, answer) => {
                self.answers.insert(token, answer);
                format!(r#"{{"told":{told},"token":{token}}}"#)
            }
            Item::Outcome(outcome) => format!(r#"{{"outcome":{outcome},"token":{token}}}"#),
        };
        self.untaken.insert(token, written);
    }

    /// The first item Python has not taken.
    fn first_untaken(&self) -> Option<String> {
        self.untaken.first_key_value().map(|(_, item)| item.clone())
    }
}

/// What a call holds of the bridge's.
struct Generation {
    /// What the first crossing begins: nothing is sent before.
    unbegun: Mutex<Option<Held<'static, ()>>>,
    items: Arc<TaskMutex<Receiver<Item>>>,
    ledger: Arc<Mutex<Ledger>>,
    /// Cancelled when Python closes the call, or no longer holds it.
    closed: CancellationToken,
    label: String,
}

impl Generation {
    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        locked(&self.ledger)
    }
}

fn locked(ledger: &Mutex<Ledger>) -> MutexGuard<'_, Ledger> {
    ledger.lock().unwrap_or_else(PoisonError::into_inner)
}

#[pyclass(name = "_ImageCall", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonImageCall {
    generation: Kept<Generation>,
}

impl std::fmt::Debug for PythonImageCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PythonImageCall")
            .finish_non_exhaustive()
    }
}

impl Drop for PythonImageCall {
    /// A call Python no longer holds is closed: its generation stops. A
    /// forked child stops nothing: the generation is its parent's.
    fn drop(&mut self) {
        if !fork::is_forked()
            && let Some(generation) = self.generation.get()
        {
            generation.closed.cancel();
        }
    }
}

impl PythonImageCall {
    fn of(provider: HeldImages, label: &str, request_json: &str) -> Result<Self, Failure> {
        let asked: Asked = serde_json::from_str(request_json)
            .map_err(|error| Failure::refused(&error.to_string(), label))?;
        // The items a generation is ahead of Python by: past them, it waits
        // for Python to take some.
        let (sender, receiver) = mpsc::channel(64);
        let closed = CancellationToken::new();
        let work = generated(provider, asked, sender, label.to_owned());
        let stopping = closed.clone();
        let unbegun: Held<'static, ()> = Box::pin(async move {
            // Closed, the generation is dropped: its attempts in flight are
            // told unknown through the callback, which still crosses.
            let _: Option<()> = stopping.run_until_cancelled(work).await;
        });
        Ok(Self {
            generation: Kept::new(Generation {
                unbegun: Mutex::new(Some(unbegun)),
                items: Arc::new(TaskMutex::new(receiver)),
                ledger: Arc::new(Mutex::new(Ledger::default())),
                closed,
                label: label.to_owned(),
            }),
        })
    }

    fn generation(&self) -> PyResult<&Generation> {
        let held = self.generation.get();
        held.ok_or_else(|| PyRuntimeError::new_err("the image call was released"))
    }
}

#[pymethods]
impl PythonImageCall {
    /// The first item of the call Python has not taken, as JSON, with its
    /// token: an attempt told, which the host's answer goes back with, or
    /// the outcome; None once the call has nothing more. An item stays the
    /// first until Python takes it (`taken`): one a cancelled crossing
    /// carried is given again. The first call begins the generation.
    fn next<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        fork::ensure_unforked()?;
        let generation = self.generation()?;
        guarded(py, &generation.label, || {
            let unbegun = generation
                .unbegun
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(work) = unbegun {
                runtime::spawn(work)?;
            }
            let items = Arc::clone(&generation.items);
            let ledger = Arc::clone(&generation.ledger);
            runtime::call(py, &generation.label, async move {
                loop {
                    if let Some(item) = locked(&ledger).first_untaken() {
                        return Ok(Some(item));
                    }
                    let mut items = items.lock().await;
                    // Another crossing may have filed one meanwhile.
                    if let Some(item) = locked(&ledger).first_untaken() {
                        return Ok(Some(item));
                    }
                    // Received and filed in one step: a crossing dropped
                    // here loses nothing.
                    match items.recv().await {
                        Some(item) => locked(&ledger).file(item),
                        None => return Ok(None),
                    }
                }
            })
        })
    }

    /// Python took the item of `token`: it is given no more.
    fn taken(&self, py: Python<'_>, token: u64) -> PyResult<()> {
        fork::ensure_unforked()?;
        let generation = self.generation()?;
        guarded(py, &generation.label, || {
            generation.ledger().untaken.remove(&token);
            Ok(())
        })
    }

    /// The host's answer to the attempt told with `token`: none for a
    /// callback that returned, the words of its failure for one that
    /// raised.
    #[pyo3(signature = (token, failed))]
    fn reply(&self, py: Python<'_>, token: u64, failed: Option<String>) -> PyResult<()> {
        fork::ensure_unforked()?;
        let generation = self.generation()?;
        guarded(py, &generation.label, || {
            let answer = generation.ledger().answers.remove(&token);
            if let Some(answer) = answer {
                // The attempt dropped meanwhile waits for nothing.
                drop(answer.send(failed.map_or(Ok(()), Err)));
            }
            Ok(())
        })
    }

    /// Stops the generation: its requests are dropped, and its attempts in
    /// flight are told unknown, which `next` still gives.
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let Ok(generation) = self.generation() else {
            return Ok(());
        };
        guarded(py, &generation.label, || {
            generation.closed.cancel();
            drop(
                generation
                    .unbegun
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take(),
            );
            Ok(())
        })
    }
}

/// What an image provider says of itself, read once, when Python is given
/// it.
#[derive(Debug)]
struct Said {
    name: String,
    label: String,
    model_name: String,
    supports_editing: bool,
    catalog_entry: Option<String>,
}

#[pyclass(name = "_ImageProvider", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonImageProvider {
    provider: Kept<HeldImages>,
    said: Said,
}

impl std::fmt::Debug for PythonImageProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PythonImageProvider")
            .field("provider", &self.said.name)
            .field("model", &self.said.model_name)
            .finish_non_exhaustive()
    }
}

impl PythonImageProvider {
    /// `provider` as Python holds it, with what it says of itself, and
    /// `label`, the name its vendor states its failures carry.
    pub(crate) fn holding(provider: impl ImageProvider, label: &str) -> Result<Self, Failure> {
        let provider: HeldImages = Arc::new(provider);
        let entry = provider.dyn_catalog_entry();
        let entry = entry.map(|entry| written(&entry, "the catalogue's entry"));
        let said = Said {
            name: provider.dyn_name().to_owned(),
            label: label.to_owned(),
            model_name: provider.dyn_model_name().to_owned(),
            supports_editing: provider.dyn_supports_editing(),
            catalog_entry: entry.transpose()?,
        };
        Ok(Self {
            provider: Kept::new(provider),
            said,
        })
    }

    /// The name the provider goes by.
    pub(crate) fn name(&self) -> &str {
        &self.said.name
    }

    fn held(&self) -> PyResult<&HeldImages> {
        let held = self.provider.get();
        held.ok_or_else(|| PyRuntimeError::new_err("the provider was released"))
    }
}

#[pymethods]
impl PythonImageProvider {
    #[getter(name)]
    fn said_name(&self) -> &str {
        &self.said.name
    }

    /// The label the provider's failures carry.
    #[getter]
    fn label(&self) -> &str {
        &self.said.label
    }

    #[getter]
    fn model_name(&self) -> &str {
        &self.said.model_name
    }

    #[getter]
    const fn supports_editing(&self) -> bool {
        self.said.supports_editing
    }

    /// The catalogue's entry for the configured model as JSON, or None.
    fn catalog_entry_json(&self) -> Option<&str> {
        self.said.catalog_entry.as_deref()
    }

    /// The call of one generation, which `request_json` asks: nothing is
    /// sent before its first `next()`.
    fn generation(&self, py: Python<'_>, request_json: &str) -> PyResult<PythonImageCall> {
        guarded(py, &self.said.label, || {
            fork::ensure_unforked()?;
            let provider = Arc::clone(self.held()?);
            PythonImageCall::of(provider, &self.said.label, request_json)
                .map_err(|failure| failure.raised(py))
        })
    }

    /// Releases what the provider holds, as Rust's `close()`.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let label = self.said.label.clone();
        guarded(py, &label, || {
            let provider = Arc::clone(self.held()?);
            runtime::call(py, &label, async move {
                provider.dyn_close().await;
                Ok(())
            })
        })
    }
}
