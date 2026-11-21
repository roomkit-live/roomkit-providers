//! A scripted provider, for the tests of the crossing: it speaks no wire
//! and belongs to no vendor.
//!
//! It is the contract's `MockAiProvider`, which answers from a list and
//! keeps each request it was asked, with what the tests of the crossing
//! need beside: a failure it raises, an answer that never comes and is seen
//! to be dropped, and deferred tools it says it supports.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, MockAiProvider, MockAiProviderOptions, ModelInfo,
    ProviderError, ProviderErrorOptions, ProviderStream, TextStream, Yielder, items, next,
    request_api_key,
};
use serde::Deserialize;
use tokio::sync::watch;

use super::errors::Failure;
use super::provider::{PythonProvider, written};
use super::runtime;

/// What a scripted provider does, as its test writes it.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[expect(clippy::struct_excessive_bools, reason = "the mock's own switches")]
struct Script {
    /// The whole responses it answers, round-robin.
    responses: Vec<AiResponse>,
    vision: bool,
    streaming: bool,
    tool_call_delta_chunks: i64,
    response_schema: bool,
    response_schema_with_tools: bool,
    deferred_tools: bool,
    /// The failure every call raises.
    failure: Option<Refusal>,
    /// Whether a generation never answers.
    silent: bool,
}

/// A provider's failure, attribute by attribute.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Refusal {
    message: String,
    retryable: bool,
    provider: String,
    status_code: Option<u16>,
    context_overflow: Option<bool>,
    before_request: bool,
}

impl From<Refusal> for ProviderError {
    fn from(refusal: Refusal) -> Self {
        let options = ProviderErrorOptions {
            retryable: refusal.retryable,
            provider: refusal.provider,
            status_code: refusal.status_code,
            context_overflow: refusal.context_overflow,
            before_request: refusal.before_request,
        };
        Self::new(refusal.message, options)
    }
}

/// How many generations that never answer began, and how many of them
/// were dropped.
#[derive(Clone, Copy, Debug, Default)]
struct Seen {
    begun: usize,
    dropped: usize,
}

/// Counts a generation that never answers as dropped, once it is.
struct Waiting(Arc<watch::Sender<Seen>>);

impl Drop for Waiting {
    fn drop(&mut self) {
        self.0
            .send_modify(|seen| seen.dropped = seen.dropped.saturating_add(1));
    }
}

#[derive(Clone, Debug)]
struct Scripted {
    mock: MockAiProvider,
    deferred_tools: bool,
    failure: Option<ProviderError>,
    silent: bool,
    seen: Arc<watch::Sender<Seen>>,
}

impl Scripted {
    fn new(script: Script) -> Self {
        let options = MockAiProviderOptions {
            ai_responses: Some(script.responses),
            vision: script.vision,
            streaming: script.streaming,
            tool_call_delta_chunks: script.tool_call_delta_chunks,
            response_schema: script.response_schema,
            response_schema_with_tools: script.response_schema_with_tools,
            ..MockAiProviderOptions::default()
        };
        Self {
            mock: MockAiProvider::new(options),
            deferred_tools: script.deferred_tools,
            failure: script.failure.map(ProviderError::from),
            silent: script.silent,
            seen: Arc::new(watch::Sender::new(Seen::default())),
        }
    }

    /// What stands before an answer: the scripted failure, or, for a
    /// provider that never answers, a wait that does not end.
    async fn before_answering(&self) -> Result<(), ProviderError> {
        if let Some(failure) = &self.failure {
            return Err(failure.clone());
        }
        if self.silent {
            self.seen
                .send_modify(|seen| seen.begun = seen.begun.saturating_add(1));
            let _waiting = Waiting(Arc::clone(&self.seen));
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

impl AiProvider for Scripted {
    fn name(&self) -> &str {
        self.mock.name()
    }

    fn model_name(&self) -> &str {
        self.mock.model_name()
    }

    fn supports_vision(&self) -> bool {
        self.mock.supports_vision()
    }

    fn available_models() -> Vec<ModelInfo> {
        MockAiProvider::available_models()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.before_answering().await?;
        Ok(Self::available_models())
    }

    fn supports_streaming(&self) -> bool {
        self.mock.supports_streaming()
    }

    fn supports_structured_streaming(&self) -> bool {
        self.mock.supports_structured_streaming()
    }

    fn supports_response_schema(&self) -> bool {
        self.mock.supports_response_schema()
    }

    fn supports_response_schema_with_tools(&self) -> bool {
        self.mock.supports_response_schema_with_tools()
    }

    fn supports_deferred_tools(&self) -> bool {
        self.deferred_tools
    }

    async fn generate(&self, request: &AiRequest) -> Result<AiResponse, ProviderError> {
        self.before_answering().await?;
        self.mock.generate(request).await
    }

    fn generate_stream<'a>(&'a self, request: &'a AiRequest) -> TextStream<'a> {
        Box::pin(items(
            move |out: Yielder<Result<String, ProviderError>>| async move {
                if let Err(failure) = self.before_answering().await {
                    return out.give(Err(failure)).await;
                }
                let mut pieces = self.mock.generate_stream(request);
                while let Some(piece) = next(&mut pieces).await {
                    out.give(piece).await;
                }
            },
        ))
    }

    fn generate_structured_stream<'a>(&'a self, request: &'a AiRequest) -> ProviderStream<'a> {
        Box::pin(items(move |out: Yielder<_>| async move {
            if let Err(failure) = self.before_answering().await {
                return out.give(Err(failure)).await;
            }
            let mut events = self.mock.generate_structured_stream(request);
            while let Some(event) = next(&mut events).await {
                out.give(event).await;
            }
        }))
    }
}

#[pyclass(name = "_Scripted", frozen, module = "roomkit_providers._native")]
#[derive(Debug)]
struct PythonScripted {
    scripted: Scripted,
}

impl PythonScripted {
    /// Resolves once `reached` says so of what the provider has seen.
    fn seen<'py>(
        &self,
        py: Python<'py>,
        reached: impl Fn(&Seen) -> bool + Send + 'static,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut seen = self.scripted.seen.subscribe();
        runtime::call(py, async move {
            // The provider holds the sender: it outlives this wait.
            drop(seen.wait_for(reached).await);
            Ok(())
        })
    }
}

#[pymethods]
impl PythonScripted {
    /// The provider, as any provider crosses.
    fn provider(&self) -> PythonProvider {
        PythonProvider::holding(self.scripted.clone())
    }

    /// The requests the provider was asked, in order, as a JSON list: each
    /// as the contract writes it, its credential masked.
    fn asked_json(&self) -> PyResult<String> {
        written(&self.scripted.mock.calls(), "the requests").map_err(Failure::raised)
    }

    /// The credential each request reached the provider with, in order, as
    /// a provider reads it: None for a request that carried none.
    fn keys(&self) -> Vec<Option<String>> {
        let asked = self.scripted.mock.calls();
        let key = |request: &AiRequest| request_api_key(request).map(str::to_owned);
        asked.iter().map(key).collect()
    }

    /// Resolves once `count` generations that never answer have begun.
    fn begun<'py>(&self, py: Python<'py>, count: usize) -> PyResult<Bound<'py, PyAny>> {
        self.seen(py, move |seen| seen.begun >= count)
    }

    /// Resolves once `count` generations that never answer were dropped.
    fn dropped<'py>(&self, py: Python<'py>, count: usize) -> PyResult<Bound<'py, PyAny>> {
        self.seen(py, move |seen| seen.dropped >= count)
    }
}

/// The scripted provider `script_json` writes.
#[pyfunction]
fn _scripted(script_json: &str) -> PyResult<PythonScripted> {
    let script: Script = serde_json::from_str(script_json)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    Ok(PythonScripted {
        scripted: Scripted::new(script),
    })
}

/// Adds the scripted provider to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(_scripted, module)?)?;
    module.add_class::<PythonScripted>()
}
