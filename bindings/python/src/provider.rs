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

use pyo3::prelude::*;
use roomkit_providers_llm::{
    API_KEY_METADATA_KEY, AiProvider, AiRequest, DynAiProvider, ProviderError,
};
use serde::Serialize;
use serde_json::Value;

use super::errors::Failure;
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
/// the contract refuses fails as a provider refuses one before sending it.
pub(crate) fn request_of(json: &str, api_key: Option<String>) -> Result<AiRequest, Failure> {
    let mut request: AiRequest =
        serde_json::from_str(json).map_err(|error| Failure::refused(&error.to_string()))?;
    if let Some(key) = api_key {
        request
            .metadata
            .insert(API_KEY_METADATA_KEY.to_owned(), Value::String(key));
    }
    Ok(request)
}

#[pyclass(name = "_Provider", frozen, module = "roomkit_providers._native")]
pub(crate) struct PythonProvider {
    provider: Held,
}

impl PythonProvider {
    /// `provider` as Python holds it.
    pub(crate) fn holding(provider: impl AiProvider) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }
}

impl fmt::Debug for PythonProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PythonProvider")
            .field("provider", &self.provider.dyn_type_name())
            .field("model", &self.provider.dyn_model_name())
            .finish()
    }
}

#[pymethods]
impl PythonProvider {
    #[getter]
    fn name(&self) -> &str {
        self.provider.dyn_name()
    }

    #[getter]
    fn model_name(&self) -> &str {
        self.provider.dyn_model_name()
    }

    #[getter]
    fn supports_vision(&self) -> bool {
        self.provider.dyn_supports_vision()
    }

    #[getter]
    fn supports_streaming(&self) -> bool {
        self.provider.dyn_supports_streaming()
    }

    #[getter]
    fn supports_structured_streaming(&self) -> bool {
        self.provider.dyn_supports_structured_streaming()
    }

    #[getter]
    fn supports_response_schema(&self) -> bool {
        self.provider.dyn_supports_response_schema()
    }

    #[getter]
    fn supports_response_schema_with_tools(&self) -> bool {
        self.provider.dyn_supports_response_schema_with_tools()
    }

    #[getter]
    fn supports_deferred_tools(&self) -> bool {
        self.provider.dyn_supports_deferred_tools()
    }

    #[getter]
    fn context_window(&self) -> Option<u64> {
        self.provider.dyn_context_window()
    }

    /// The catalogue's entry for the configured model as JSON, or None.
    fn catalog_entry_json(&self) -> PyResult<Option<String>> {
        let entry = self.provider.dyn_catalog_entry();
        let entry = entry.map(|entry| written(&entry, "the catalogue's entry"));
        entry.transpose().map_err(Failure::raised)
    }

    /// The models the provider lists, as a JSON list.
    fn list_models<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        runtime::call(py, async move {
            let listed = provider.dyn_list_models().await.map_err(Failure::from);
            let listed = listed.and_then(|models| written(&models, "the model listing"));
            listed.map_err(Failure::raised)
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
        let provider = Arc::clone(&self.provider);
        runtime::call(py, async move {
            let answered = async {
                let request = request_of(&request_json, api_key)?;
                let response = provider.dyn_generate(&request).await?;
                written(&response, "the response")
            };
            answered.await.map_err(Failure::raised)
        })
    }

    /// The stream of the events of one generation answering the request
    /// `request_json` writes, with `api_key` as `generate` takes it.
    /// Nothing is read or sent before the stream's first `next()`.
    #[pyo3(signature = (request_json, api_key))]
    fn stream(&self, request_json: String, api_key: Option<String>) -> PyResult<PythonStream> {
        PythonStream::of(Arc::clone(&self.provider), request_json, api_key)
    }

    /// Releases what the provider holds, as Rust's `close()`.
    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let provider = Arc::clone(&self.provider);
        runtime::call(py, async move {
            provider.dyn_close().await;
            Ok(())
        })
    }
}
