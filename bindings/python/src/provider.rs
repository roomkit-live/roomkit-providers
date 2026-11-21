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
use roomkit_providers_llm::{
    API_KEY_METADATA_KEY, AiProvider, AiRequest, DynAiProvider, ProviderError,
};
use serde::Serialize;
use serde_json::Value;

use super::errors::Failure;
use super::fork::Kept;
use super::runtime;
use super::stream::PythonStream;

/// A provider of the contract, whichever its type.
pub(crate) type Held = Arc<dyn DynAiProvider>;

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
) -> Result<AiRequest, Failure> {
    let mut request: AiRequest =
        serde_json::from_str(json).map_err(|error| Failure::refused(&error.to_string(), label))?;
    if let Some(key) = api_key {
        request
            .metadata
            .insert(API_KEY_METADATA_KEY.to_owned(), Value::String(key));
    }
    Ok(request)
}

#[pyclass(name = "_Provider", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonProvider {
    provider: Kept<Held>,
}

impl PythonProvider {
    /// `provider` as Python holds it.
    pub(crate) fn holding(provider: impl AiProvider) -> Self {
        Self {
            provider: Kept::new(Arc::new(provider)),
        }
    }

    /// The provider, which the object holds as long as it lives.
    fn held(&self) -> PyResult<&Held> {
        let held = self.provider.get();
        held.ok_or_else(|| PyRuntimeError::new_err("the provider was released"))
    }
}

impl fmt::Debug for PythonProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let provider = self.provider.get();
        formatter
            .debug_struct("PythonProvider")
            .field("provider", &provider.map(|held| held.dyn_type_name()))
            .field("model", &provider.map(|held| held.dyn_model_name()))
            .finish()
    }
}

#[pymethods]
impl PythonProvider {
    #[getter]
    fn name(&self) -> PyResult<&str> {
        Ok(self.held()?.dyn_name())
    }

    /// The label the provider's failures carry.
    #[getter]
    fn label(&self) -> PyResult<&str> {
        Ok(self.held()?.dyn_label())
    }

    #[getter]
    fn model_name(&self) -> PyResult<&str> {
        Ok(self.held()?.dyn_model_name())
    }

    #[getter]
    fn supports_vision(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_vision())
    }

    #[getter]
    fn supports_streaming(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_streaming())
    }

    #[getter]
    fn supports_structured_streaming(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_structured_streaming())
    }

    #[getter]
    fn supports_response_schema(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_response_schema())
    }

    #[getter]
    fn supports_response_schema_with_tools(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_response_schema_with_tools())
    }

    #[getter]
    fn supports_deferred_tools(&self) -> PyResult<bool> {
        Ok(self.held()?.dyn_supports_deferred_tools())
    }

    #[getter]
    fn context_window(&self) -> PyResult<Option<u64>> {
        Ok(self.held()?.dyn_context_window())
    }

    /// The catalogue's entry for the configured model as JSON, or None.
    fn catalog_entry_json(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let entry = self.held()?.dyn_catalog_entry();
        let entry = entry.map(|entry| written(&entry, "the catalogue's entry"));
        entry.transpose().map_err(|failure| failure.raised(py))
    }

    /// The models the provider lists, as a JSON list.
    fn list_models<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(self.held()?);
        let label = provider.dyn_label().to_owned();
        runtime::call(py, &label, async move {
            let listed = provider.dyn_list_models().await?;
            written(&listed, "the model listing")
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
        let provider = Arc::clone(self.held()?);
        let label = provider.dyn_label().to_owned();
        runtime::call(py, &label, async move {
            let request = request_of(&request_json, api_key, provider.dyn_label())?;
            let response = provider.dyn_generate(&request).await?;
            written(&response, "the response")
        })
    }

    /// The stream of the events of one generation answering the request
    /// `request_json` writes, with `api_key` as `generate` takes it.
    /// Nothing is read or sent before the stream's first `next()`.
    #[pyo3(signature = (request_json, api_key))]
    fn stream(&self, request_json: String, api_key: Option<String>) -> PyResult<PythonStream> {
        PythonStream::of(Arc::clone(self.held()?), request_json, api_key)
    }

    /// Releases what the provider holds, as Rust's `close()`.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(self.held()?);
        let label = provider.dyn_label().to_owned();
        runtime::call(py, &label, async move {
            provider.dyn_close().await;
            Ok(())
        })
    }
}
