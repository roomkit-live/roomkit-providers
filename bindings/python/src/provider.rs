//! Any provider of the contract as Python holds it: what it sends and reads
//! stays in its own crate.
//!
//! JSON text crosses, both ways, in the contract's forms: a request as one
//! text, a whole answer as one text, and a stream, at each crossing, as the
//! list of every item already received. The credential a host lends one
//! request never crosses inside the JSON text, where it would be written
//! masked: it crosses apart, and is put back where a provider reads it.

use std::fmt;
use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use roomkit_ai::{API_KEY_METADATA_KEY, AiContext, AiProvider, ProviderError};
use roomkit_providers_python_forms::Form;
use serde::Serialize;
use serde_json::Value;

use super::dynamic::AnyProvider;
use super::errors::{Failure, guarded};
use super::fork::Kept;
use super::runtime;
use super::stream::PythonStream;

/// Optional lifecycle of a service that belongs to the provider.
pub(crate) trait Managed: Send + Sync {
    fn base_url(&self) -> String;
    fn start(
        &self,
    ) -> std::pin::Pin<Box<dyn Future<Output = Result<(), ProviderError>> + Send + '_>>;
}

/// A provider of the contract, whichever its type.
pub(crate) type Held = Arc<dyn AnyProvider>;

/// `value` as JSON text.
pub(crate) fn written<T: Serialize>(value: &T, what: &str) -> Result<String, Failure> {
    serde_json::to_string(value).map_err(|_| {
        let said = format!("{what} could not be written as JSON");
        Failure::from(ProviderError::from(said))
    })
}

/// The request `json` writes, in the contract's form, with `api_key`, the
/// credential of this one request, where a provider reads it. A request
/// the contract refuses fails as the provider labelled `label` refuses one
/// before sending it.
pub(crate) fn request_of(
    json: &str,
    api_key: Option<String>,
    label: &str,
) -> Result<AiContext, Failure> {
    let read = serde_json::from_str(json).and_then(AiContext::read);
    let mut request = read.map_err(|error| Failure::refused(&error.to_string(), label))?;
    if let Some(key) = api_key {
        request
            .metadata
            .insert(API_KEY_METADATA_KEY.to_owned(), Value::String(key));
    }
    Ok(request)
}

/// What a provider says of itself, read once, when Python is given it:
/// asking again runs nothing of the provider.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "what a provider says it supports, one answer each"
)]
struct Said {
    name: String,
    label: String,
    model_name: String,
    supports_vision: bool,
    supports_streaming: bool,
    supports_structured_streaming: bool,
    supports_response_schema: bool,
    supports_response_schema_with_tools: bool,
    supports_deferred_tools: bool,
    context_window: Option<u64>,
    /// The catalogue's entry for the configured model, as JSON.
    catalog_entry: Option<String>,
}

impl Said {
    fn of(provider: &Held, label: &str) -> Result<Self, Failure> {
        let entry = provider.dyn_catalog_entry();
        let entry = entry.map(|entry| written(&entry, "the catalogue's entry"));
        Ok(Self {
            name: provider.dyn_name().to_owned(),
            label: label.to_owned(),
            model_name: provider.dyn_model_name().to_owned(),
            supports_vision: provider.dyn_supports_vision(),
            supports_streaming: provider.dyn_supports_streaming(),
            supports_structured_streaming: provider.dyn_supports_structured_streaming(),
            supports_response_schema: provider.dyn_supports_response_schema(),
            supports_response_schema_with_tools: provider.dyn_supports_response_schema_with_tools(),
            supports_deferred_tools: provider.dyn_supports_deferred_tools(),
            context_window: provider.dyn_context_window(),
            catalog_entry: entry.transpose()?,
        })
    }
}

#[pyclass(name = "_Provider", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonProvider {
    provider: Kept<Held>,
    said: Said,
    managed: Kept<Option<Arc<dyn Managed>>>,
}

impl PythonProvider {
    /// `provider` as Python holds it, with what it says of itself, and
    /// `label`, the name its vendor states its failures carry.
    pub(crate) fn holding(
        provider: impl AiProvider,
        label: &str,
        managed: Option<Arc<dyn Managed>>,
    ) -> Result<Self, Failure> {
        let provider: Held = Arc::new(provider);
        let said = Said::of(&provider, label)?;
        Ok(Self {
            provider: Kept::new(provider),
            said,
            managed: Kept::new(managed),
        })
    }

    fn managed(&self) -> PyResult<&Arc<dyn Managed>> {
        self.managed
            .get()
            .and_then(Option::as_ref)
            .ok_or_else(|| PyRuntimeError::new_err("this provider has no managed server"))
    }

    /// The name the provider goes by.
    pub(crate) fn name(&self) -> &str {
        &self.said.name
    }

    /// The label the provider's failures carry.
    pub(crate) fn label(&self) -> &str {
        &self.said.label
    }

    /// The provider, which the object holds as long as it lives.
    fn held(&self) -> PyResult<&Held> {
        let held = self.provider.get();
        held.ok_or_else(|| PyRuntimeError::new_err("the provider was released"))
    }
}

impl fmt::Debug for PythonProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PythonProvider")
            .field("provider", &self.said.name)
            .field("model", &self.said.model_name)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PythonProvider {
    #[getter(name)]
    fn said_name(&self) -> &str {
        &self.said.name
    }

    /// The label the provider's failures carry.
    #[getter(label)]
    fn said_label(&self) -> &str {
        &self.said.label
    }

    #[getter]
    fn model_name(&self) -> &str {
        &self.said.model_name
    }

    #[getter]
    const fn supports_vision(&self) -> bool {
        self.said.supports_vision
    }

    #[getter]
    const fn supports_streaming(&self) -> bool {
        self.said.supports_streaming
    }

    #[getter]
    const fn supports_structured_streaming(&self) -> bool {
        self.said.supports_structured_streaming
    }

    #[getter]
    const fn supports_response_schema(&self) -> bool {
        self.said.supports_response_schema
    }

    #[getter]
    const fn supports_response_schema_with_tools(&self) -> bool {
        self.said.supports_response_schema_with_tools
    }

    #[getter]
    const fn supports_deferred_tools(&self) -> bool {
        self.said.supports_deferred_tools
    }

    #[getter]
    const fn context_window(&self) -> Option<u64> {
        self.said.context_window
    }

    /// The catalogue's entry for the configured model as JSON, or None.
    fn catalog_entry_json(&self) -> Option<&str> {
        self.said.catalog_entry.as_deref()
    }

    /// The models the provider lists, as a JSON list.
    fn list_models<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let label = self.label();
        guarded(py, label, || {
            let provider = Arc::clone(self.held()?);
            runtime::call(py, label, async move {
                let listed = provider.dyn_list_models().await?;
                written(&listed, "the model listing")
            })
        })
    }

    /// One whole answer to the request `request_json` writes, as JSON.
    /// `api_key` is the credential of this one request, when its host lent
    /// one.
    #[pyo3(signature = (request_json, api_key))]
    fn generate<'py>(
        &self,
        py: Python<'py>,
        request_json: String,
        api_key: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let label = self.label();
        guarded(py, label, || {
            let provider = Arc::clone(self.held()?);
            let labelled = label.to_owned();
            runtime::call(py, label, async move {
                let lent = api_key.clone();
                let answered = async {
                    let request = request_of(&request_json, api_key, &labelled)?;
                    let response = provider.dyn_generate(&request).await?;
                    written(&response.written(), "the response")
                };
                let answered: Result<String, Failure> = answered.await;
                answered.map_err(|failure| failure.without(lent.as_deref()))
            })
        })
    }

    /// The stream of the events of one generation answering the request
    /// `request_json` writes, with `api_key` as `generate` takes it.
    /// Nothing is read or sent before the stream's first `next()`.
    #[pyo3(signature = (request_json, api_key))]
    fn stream(
        &self,
        py: Python<'_>,
        request_json: String,
        api_key: Option<String>,
    ) -> PyResult<PythonStream> {
        guarded(py, self.label(), || {
            PythonStream::of(
                Arc::clone(self.held()?),
                self.label(),
                request_json,
                api_key,
            )
        })
    }

    /// The managed server's local endpoint.
    fn managed_base_url(&self) -> PyResult<String> {
        Ok(self.managed()?.base_url())
    }

    /// Await startup on the binding's runtime so cancelling the awaitable stops it.
    fn managed_start<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let label = self.label();
        guarded(py, label, || {
            let provider = Arc::clone(self.managed()?);
            runtime::call(py, label, async move {
                provider.start().await.map_err(Failure::from)
            })
        })
    }

    /// Releases what the provider holds, as Rust's `close()`.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let label = self.label();
        guarded(py, label, || {
            let provider = Arc::clone(self.held()?);
            runtime::call(py, label, async move {
                provider.dyn_close().await;
                Ok(())
            })
        })
    }
}
