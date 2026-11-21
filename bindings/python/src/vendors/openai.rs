//! `OpenAI`'s part: its configuration, how its provider is built, and its
//! catalogue.

use std::net::{IpAddr, SocketAddr};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use roomkit_providers_llm::AiProvider;
use roomkit_providers_openai::{OpenAi, OpenAiConfig, ReplayWaits};

use crate::errors::Failure;
use crate::provider::{PythonProvider, written};

/// The configuration `json` writes, in the form of Python's `OpenAIConfig`.
fn config_of(json: &str) -> PyResult<OpenAiConfig> {
    serde_json::from_str(json).map_err(|error| PyValueError::new_err(error.to_string()))
}

/// The provider of the configuration `config_json` writes, with `api_key`
/// as its key: the key crosses apart, never inside the JSON text.
fn built(config_json: &str, api_key: &str) -> PyResult<OpenAi> {
    let mut config = config_of(config_json)?;
    api_key.clone_into(&mut config.api_key);
    OpenAi::new(config).map_err(|error| Failure::from(error).raised())
}

/// The provider of the configuration `config_json` writes, with `api_key`
/// as its key.
#[pyfunction]
fn openai(config_json: &str, api_key: &str) -> PyResult<PythonProvider> {
    built(config_json, api_key).map(PythonProvider::holding)
}

/// The JSON form of the configuration `config_json` writes, read and
/// written back: what the reader refuses is a `ValueError`, a member left
/// out takes its default, and the two switches a modern model decides on
/// `OpenAI`'s own endpoint are written decided. The key is written masked.
#[pyfunction]
fn openai_config_form(config_json: &str) -> PyResult<String> {
    let config = config_of(config_json)?;
    written(&config, "the configuration").map_err(Failure::raised)
}

/// The curated catalogue as a JSON list, with no request.
#[pyfunction]
fn openai_available_models_json() -> PyResult<String> {
    let models = OpenAi::available_models();
    written(&models, "the catalogue").map_err(Failure::raised)
}

/// The waits of a provider a replay drives, which takes none.
#[pyclass(
    name = "_OpenAIReplayWaits",
    frozen,
    module = "roomkit_providers._native"
)]
#[derive(Debug)]
struct PythonReplayWaits {
    waits: ReplayWaits,
}

#[pymethods]
impl PythonReplayWaits {
    /// The waits, in seconds, the provider did not take between its tries.
    fn taken(&self) -> Vec<f64> {
        self.waits.taken()
    }
}

/// For the replay of the recorded cases, never for a host: the provider
/// [`openai`] builds, sending to `host:port`, a loopback address, over
/// plain HTTP, reading a `retry-after` date against `clock` and keeping its
/// waits instead of taking them.
#[pyfunction]
fn _openai_replaying(
    config_json: &str,
    api_key: &str,
    host: &str,
    port: u16,
    clock: f64,
) -> PyResult<(PythonProvider, PythonReplayWaits)> {
    let host: IpAddr = host
        .parse()
        .map_err(|_| PyValueError::new_err("a replay's fake is named by its IP address"))?;
    let replaying = built(config_json, api_key)?.replaying(SocketAddr::new(host, port), clock);
    let (provider, waits) = replaying.map_err(|error| Failure::from(error).raised())?;
    Ok((
        PythonProvider::holding(provider),
        PythonReplayWaits { waits },
    ))
}

/// Adds `OpenAI`'s functions to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(openai, module)?)?;
    module.add_function(wrap_pyfunction!(openai_config_form, module)?)?;
    module.add_function(wrap_pyfunction!(openai_available_models_json, module)?)?;
    module.add_function(wrap_pyfunction!(_openai_replaying, module)?)?;
    module.add_class::<PythonReplayWaits>()
}
