//! A scripted provider, for the tests of the crossing: it speaks no wire
//! and belongs to no vendor.
//!
//! It is the contract's `MockAiProvider`, which answers from a list and
//! keeps each request it was asked, with what the tests of the crossing
//! need beside: a failure it raises, an answer that never comes and is seen
//! to be dropped, deferred tools it says it supports, and the name and the
//! label a test gives it.
//!
//! It is also a vendor of no vendor ([`VENDOR`]), so that what every
//! vendor's entries do is tested with none: a script is its configuration,
//! which may refuse to be built, or panic as it is.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use roomkit_providers_client::ReplayWaits;
use roomkit_providers_llm::{
    AiProvider, AiRequest, AiResponse, MockAiProvider, MockAiProviderOptions, ModelInfo,
    ProviderError, ProviderErrorOptions, ProviderStream, TextStream, Yielder, items, next,
    request_api_key,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use super::provider::{PythonProvider, written};
use super::runtime;
use super::vendor::{Entries, Vendor};

/// What a scripted provider does, as its test writes it.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[expect(clippy::struct_excessive_bools, reason = "the mock's own switches")]
pub(crate) struct Script {
    /// The key it is built with, as a configuration holds one, written
    /// masked: none for a configuration that states no key.
    #[serde(skip_serializing)]
    api_key: Option<String>,
    /// The name it goes by, and the label of its failures: the mock's own
    /// when a script names none.
    name: Option<String>,
    label: Option<String>,
    /// The model it says it reaches: the mock's own when a script names
    /// none.
    model: Option<String>,
    /// Why it cannot be built, when it cannot.
    refuses: Option<String>,
    /// Whether building it panics.
    panics_when_built: bool,
    /// The key it must be built with, when a script says: it is refused
    /// with another, or with none.
    expects_key: Option<String>,
    /// Whether it must be built with no key: it is refused with one.
    expects_no_key: bool,
    /// Members a configuration holds, which a scripted provider does
    /// nothing with.
    base_url: Option<String>,
    headers: Option<BTreeMap<String, String>>,
    /// A switch its vendor decides when a script leaves it out: on for a
    /// model whose name starts with `decided`.
    decided: Option<bool>,
    /// The whole responses it answers, round-robin.
    responses: Vec<AiResponse>,
    vision: bool,
    streaming: bool,
    /// Whether it says it streams events: as it streams, when a script
    /// does not say.
    structured_streaming: Option<bool>,
    tool_call_delta_chunks: i64,
    response_schema: bool,
    response_schema_with_tools: bool,
    deferred_tools: bool,
    /// The failure every call raises.
    failure: Option<Refusal>,
    /// Whether a generation never answers.
    silent: bool,
    /// Whether a whole answer and a listing panic.
    panics: bool,
    /// After how many events a stream panics.
    panics_after: Option<usize>,
    /// After how many events a stream fails as one that was cut: what came
    /// is handed on, then a failure worth a new request.
    cut_after: Option<usize>,
}

/// A provider's failure, attribute by attribute.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// How many scripted providers were released: each counts once, when the
/// last holder of it lets go.
static RELEASED: AtomicUsize = AtomicUsize::new(0);

/// Counts a scripted provider as released, once it is.
#[derive(Debug)]
struct Released;

impl Drop for Released {
    fn drop(&mut self) {
        RELEASED.fetch_add(1, Ordering::AcqRel);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Scripted {
    mock: MockAiProvider,
    _released: Arc<Released>,
    name: Option<String>,
    label: String,
    model: Option<String>,
    structured_streaming: Option<bool>,
    deferred_tools: bool,
    failure: Option<ProviderError>,
    silent: bool,
    panics: bool,
    panics_after: Option<usize>,
    cut_after: Option<usize>,
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
            _released: Arc::new(Released),
            name: script.name,
            label: script.label.unwrap_or_default(),
            model: script.model,
            structured_streaming: script.structured_streaming,
            deferred_tools: script.deferred_tools,
            failure: script.failure.map(ProviderError::from),
            silent: script.silent,
            panics: script.panics,
            panics_after: script.panics_after,
            cut_after: script.cut_after,
            seen: Arc::new(watch::Sender::new(Seen::default())),
        }
    }

    /// The failure of a stream that was cut: worth a new request.
    fn cut(&self) -> ProviderError {
        let options = ProviderErrorOptions {
            retryable: true,
            provider: self.label.clone(),
            ..ProviderErrorOptions::default()
        };
        ProviderError::new("the provider's stream was cut", options)
    }

    /// What stands before an answer: the scripted failure, or, for a
    /// provider that never answers, a wait that does not end.
    async fn before_answering(&self) -> Result<(), ProviderError> {
        if self.panics {
            stops("before it answered");
        }
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

/// What a provider with a defect does: it panics, `at` saying where.
#[expect(clippy::panic, reason = "the defect the tests of the crossing script")]
fn stops(at: &str) -> ! {
    panic!("the scripted provider panics {at}")
}

impl AiProvider for Scripted {
    fn name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.mock.name())
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn model_name(&self) -> &str {
        self.model
            .as_deref()
            .unwrap_or_else(|| self.mock.model_name())
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
        self.structured_streaming
            .unwrap_or_else(|| self.mock.supports_structured_streaming())
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
            let mut given = 0_usize;
            while let Some(event) = next(&mut events).await {
                if self.panics_after == Some(given) {
                    stops("while its stream is read");
                }
                if self.cut_after == Some(given) {
                    return out.give(Err(self.cut())).await;
                }
                out.give(event).await;
                given = given.saturating_add(1);
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
        runtime::call(py, "", async move {
            // The provider holds the sender: it outlives this wait.
            drop(seen.wait_for(reached).await);
            Ok(())
        })
    }
}

#[pymethods]
impl PythonScripted {
    /// The provider, as any provider crosses.
    fn provider(&self, py: Python<'_>) -> PyResult<PythonProvider> {
        let held = PythonProvider::holding(self.scripted.clone());
        held.map_err(|failure| failure.raised(py))
    }

    /// The requests the provider was asked, in order, as a JSON list: each
    /// as the contract writes it, its credential masked.
    fn asked_json(&self, py: Python<'_>) -> PyResult<String> {
        let asked = written(&self.scripted.mock.calls(), "the requests");
        asked.map_err(|failure| failure.raised(py))
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

/// How many scripted providers were released since the module was loaded.
/// It takes no lock: a forked child may ask.
#[pyfunction]
fn _released() -> usize {
    RELEASED.load(Ordering::Acquire)
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

/// A script read as a vendor reads its configuration: the switch it leaves
/// out is decided from its model, on for a model whose name starts with
/// `decided`. Reading one whose model's name starts with `panics` panics.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(crate) struct Configured(Script);

impl<'de> Deserialize<'de> for Configured {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut script = Script::deserialize(deserializer)?;
        let model = script.model.as_deref().unwrap_or_default();
        if model.starts_with("panics") {
            stops("as its configuration is read");
        }
        script.decided = script.decided.or(Some(model.starts_with("decided")));
        Ok(Self(script))
    }
}

/// The scripted provider as a vendor: its script is its configuration.
#[derive(Debug)]
pub(crate) struct ScriptedVendor;

/// The label of a scripted provider built as a vendor's is.
const LABEL: &str = "scripted";

impl Vendor for ScriptedVendor {
    type Config = Configured;
    type Provider = Scripted;

    const NAME: &'static str = "scripted";
    const RECORDED_AS: &'static str = "MockAIProvider";
    const LABEL: &'static str = LABEL;

    fn keyed(mut config: Configured, key: Option<String>) -> Configured {
        config.0.api_key = key;
        config
    }

    fn built(Configured(mut config): Configured) -> Result<Scripted, ProviderError> {
        if config.panics_when_built {
            stops("as it is built");
        }
        let another = config
            .expects_key
            .as_ref()
            .is_some_and(|expected| config.api_key.as_ref() != Some(expected));
        let wrong_key = another || (config.expects_no_key && config.api_key.is_some());
        let refused = if wrong_key {
            Some("the key did not cross as it was given".to_owned())
        } else {
            config.refuses.take()
        };
        if let Some(refused) = refused {
            let options = ProviderErrorOptions {
                provider: LABEL.to_owned(),
                before_request: true,
                ..ProviderErrorOptions::default()
            };
            return Err(ProviderError::new(refused, options));
        }
        config.label.get_or_insert_with(|| LABEL.to_owned());
        Ok(Scripted::new(config))
    }

    fn replaying(
        _provider: Scripted,
        _fake: SocketAddr,
        _clock: f64,
    ) -> Result<(Scripted, ReplayWaits), ProviderError> {
        Err(ProviderError::from(
            "a scripted provider has no wire to replay".to_owned(),
        ))
    }
}

/// The entries of the scripted provider, beside the vendors'.
pub(crate) static VENDOR: Entries = Entries::of::<ScriptedVendor>();

/// Adds the scripted provider to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(_scripted, module)?)?;
    module.add_function(wrap_pyfunction!(_released, module)?)?;
    module.add_class::<PythonScripted>()
}
