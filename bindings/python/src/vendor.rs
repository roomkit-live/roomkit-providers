//! What a vendor gives the native module, and the entries every vendor gets
//! for it.
//!
//! A vendor's module of `vendors` states what is its own and nothing else:
//! its configuration, how its provider is built from it, and the names
//! `RoomKit` knows the provider by ([`Vendor`]). Every entry Python calls
//! for a vendor is written here, once: the provider built from the JSON form
//! of its configuration, the key crossing apart and opened here; that JSON
//! form read and written back; the catalogue; and the provider a replay of
//! recorded cases drives. So what protects an entry is written once too: a
//! key no frame of Python holds in clear when a provider is refused, a
//! panic that leaves as the provider's failure, and a provider that names
//! itself as `RoomKit` records it.
//!
//! The key is optional here: a provider whose configuration states none, or
//! takes none at all, is built by the same entries, and its vendor says
//! what no key means to it ([`Vendor::keyed`]).

use std::net::{IpAddr, SocketAddr};

use pyo3::exceptions::PyValueError;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::PyString;
use roomkit_ai::{AiProvider, ProviderError};
use roomkit_providers_client::ReplayWaits;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::errors::{Failure, guarded};
use super::provider::{PythonProvider, written};
use super::vendors;

/// A vendor's own part: what its module of `vendors` writes.
pub(crate) trait Vendor: 'static {
    /// The provider's configuration, read and written in the JSON form of
    /// its Python model.
    type Config: DeserializeOwned + Serialize;

    /// The provider.
    type Provider: AiProvider;

    /// The name the package asks for the vendor by: its module's, in the
    /// package and here.
    const NAME: &'static str;

    /// The name `RoomKit` records the provider under, which its
    /// [`name`](AiProvider::name) must answer: the class name of
    /// `RoomKit`'s own provider for the vendor.
    const RECORDED_AS: &'static str;

    /// What the provider's failures carry as their provider, which
    /// [`label`](Self::label) must answer: what the binding raises itself
    /// carries the same.
    const LABEL: &'static str;

    /// The label `provider` gives its failures.
    fn label(provider: &Self::Provider) -> &str;

    /// `config` with `key` as its key: the key crosses apart from the JSON
    /// form, which writes it masked. `None` is a configuration that states
    /// no key, or has none to state: the vendor says what that means to
    /// its provider.
    fn keyed(config: Self::Config, key: Option<String>) -> Self::Config;

    /// The provider `config` configures.
    fn built(config: Self::Config) -> Result<Self::Provider, ProviderError>;

    /// `provider` as a replay of recorded cases drives it: sending to
    /// `fake`, a loopback address, reading a `retry-after` date against
    /// `clock` and keeping its waits instead of taking them.
    fn replaying(
        provider: Self::Provider,
        fake: SocketAddr,
        clock: f64,
    ) -> Result<(Self::Provider, ReplayWaits), ProviderError>;
}

/// The entries of one vendor, as the module looks them up by its name.
#[derive(Debug)]
pub(crate) struct Entries {
    name: &'static str,
    label: &'static str,
    provider: fn(Python<'_>, &str, Option<String>) -> PyResult<PythonProvider>,
    form: fn(Python<'_>, &str) -> PyResult<String>,
    catalogue: fn(Python<'_>) -> PyResult<String>,
    replaying: Replaying,
}

/// The entry that builds the provider a replay drives.
type Replaying = fn(
    Python<'_>,
    &str,
    Option<String>,
    SocketAddr,
    f64,
) -> PyResult<(PythonProvider, ReplayWaits)>;

impl Entries {
    /// The entries of the vendor `V`.
    pub(crate) const fn of<V: Vendor>() -> Self {
        Self {
            name: V::NAME,
            label: V::LABEL,
            provider: provider_of::<V>,
            form: form_of::<V>,
            catalogue: catalogue_of::<V>,
            replaying: replaying_of::<V>,
        }
    }
}

/// The configuration `json` writes: what its reader refuses is a
/// `ValueError`.
fn config_of<V: Vendor>(json: &str) -> PyResult<V::Config> {
    serde_json::from_str(json).map_err(|error| PyValueError::new_err(error.to_string()))
}

/// `provider` as Python holds it, once it names itself as its vendor
/// states: the name `RoomKit` records and the label of its failures are
/// never left to a default.
fn held<V: Vendor>(py: Python<'_>, provider: V::Provider) -> PyResult<PythonProvider> {
    let labelled = V::label(&provider) == V::LABEL;
    let held = PythonProvider::holding(provider, V::LABEL);
    let held = held.map_err(|failure| failure.raised(py))?;
    if labelled && held.name() == V::RECORDED_AS {
        Ok(held)
    } else {
        Err(Failure::misnamed(V::LABEL).raised(py))
    }
}

fn provider_of<V: Vendor>(
    py: Python<'_>,
    config_json: &str,
    key: Option<String>,
) -> PyResult<PythonProvider> {
    let config = V::keyed(config_of::<V>(config_json)?, key);
    let provider = V::built(config).map_err(|error| Failure::from(error).raised(py))?;
    held::<V>(py, provider)
}

fn form_of<V: Vendor>(py: Python<'_>, config_json: &str) -> PyResult<String> {
    let config = config_of::<V>(config_json)?;
    written(&config, "the configuration").map_err(|failure| failure.raised(py))
}

fn catalogue_of<V: Vendor>(py: Python<'_>) -> PyResult<String> {
    let models = V::Provider::available_models();
    written(&models, "the catalogue").map_err(|failure| failure.raised(py))
}

fn replaying_of<V: Vendor>(
    py: Python<'_>,
    config_json: &str,
    key: Option<String>,
    fake: SocketAddr,
    clock: f64,
) -> PyResult<(PythonProvider, ReplayWaits)> {
    let config = V::keyed(config_of::<V>(config_json)?, key);
    let built = V::built(config).and_then(|provider| V::replaying(provider, fake, clock));
    let (provider, waits) = built.map_err(|error| Failure::from(error).raised(py))?;
    Ok((held::<V>(py, provider)?, waits))
}

/// The entries of the vendor named `vendor`.
fn entries(vendor: &str) -> PyResult<&'static Entries> {
    let held = vendors::VENDORS.iter();
    #[cfg(feature = "testing")]
    let held = held.chain(std::iter::once(&super::testing::VENDOR));
    let mut held = held;
    held.find(|entries| entries.name == vendor)
        .ok_or_else(|| PyValueError::new_err("the native module holds no provider of that name"))
}

/// The key `secret` holds: a text, or what holds one and gives it when
/// asked (pydantic's `SecretStr`), or none for a configuration that states
/// no key. It is opened here, so that no frame of Python holds it in clear
/// when the provider is refused.
fn key_of(secret: &Bound<'_, PyAny>) -> PyResult<Option<String>> {
    if secret.is_none() {
        return Ok(None);
    }
    if secret.is_instance_of::<PyString>() {
        return secret.extract().map(Some);
    }
    let opened = secret.call_method0(intern!(secret.py(), "get_secret_value"))?;
    opened.extract().map(Some)
}

/// The provider of `vendor` that the configuration `config_json` writes
/// configures, with the key `api_key` holds as its key, or with none where
/// it is `None`: the key crosses apart, never inside the JSON text.
#[pyfunction]
fn provider(
    py: Python<'_>,
    vendor: &str,
    config_json: &str,
    api_key: &Bound<'_, PyAny>,
) -> PyResult<PythonProvider> {
    let entries = entries(vendor)?;
    guarded(py, entries.label, || {
        (entries.provider)(py, config_json, key_of(api_key)?)
    })
}

/// The JSON form of the configuration `config_json` writes for `vendor`,
/// read and written back: what the reader refuses is a `ValueError`, a
/// member left out takes its default, and a switch the vendor decides from
/// the model is written decided. The key is written masked.
#[pyfunction]
fn config_form(py: Python<'_>, vendor: &str, config_json: &str) -> PyResult<String> {
    let entries = entries(vendor)?;
    guarded(py, entries.label, || (entries.form)(py, config_json))
}

/// The curated catalogue of `vendor` as a JSON list, with no request.
#[pyfunction]
fn catalogue(py: Python<'_>, vendor: &str) -> PyResult<String> {
    let entries = entries(vendor)?;
    guarded(py, entries.label, || (entries.catalogue)(py))
}

/// The waits of a provider a replay drives, which takes none.
#[pyclass(name = "_ReplayWaits", frozen, module = "roomkit_providers._native")]
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
/// [`provider`] builds, sending to `host:port`, a loopback address, over
/// plain HTTP, reading a `retry-after` date against `clock` and keeping its
/// waits instead of taking them.
#[pyfunction]
fn _replaying(
    py: Python<'_>,
    vendor: &str,
    config_json: &str,
    api_key: &Bound<'_, PyAny>,
    host: &str,
    port: u16,
    clock: f64,
) -> PyResult<(PythonProvider, PythonReplayWaits)> {
    let entries = entries(vendor)?;
    let host: IpAddr = host
        .parse()
        .map_err(|_| PyValueError::new_err("a replay's fake is named by its IP address"))?;
    guarded(py, entries.label, || {
        let fake = SocketAddr::new(host, port);
        let (provider, waits) =
            (entries.replaying)(py, config_json, key_of(api_key)?, fake, clock)?;
        Ok((provider, PythonReplayWaits { waits }))
    })
}

/// Adds the entries every vendor gets to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(provider, module)?)?;
    module.add_function(wrap_pyfunction!(config_form, module)?)?;
    module.add_function(wrap_pyfunction!(catalogue, module)?)?;
    module.add_function(wrap_pyfunction!(_replaying, module)?)?;
    module.add_class::<PythonReplayWaits>()
}
