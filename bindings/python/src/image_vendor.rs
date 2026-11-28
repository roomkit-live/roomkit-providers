//! What an image vendor gives the native module, and the entries every
//! image vendor gets for it: as `vendor` does for the conversational ones.
//!
//! An image vendor's module of `vendors` states its configuration, how its
//! provider is built from it, and the names `RoomKit` knows it by
//! ([`ImageVendor`]). Every entry Python calls is written here once: the
//! provider built from the JSON form of its configuration, the key crossing
//! apart and opened here; the catalogue; and the provider a replay drives.

use std::net::{IpAddr, SocketAddr};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use roomkit_ai::ProviderError;
use roomkit_ai::image::ImageProvider;
use roomkit_providers_client::ReplayWaits;
use serde::de::DeserializeOwned;

use super::errors::{Failure, guarded};
use super::image::PythonImageProvider;
use super::provider::written;
use super::vendor::{PythonReplayWaits, key_of};
use super::vendors;

/// An image vendor's own part: what its module of `vendors` writes.
pub(crate) trait ImageVendor: 'static {
    /// The provider's configuration, read in the JSON form of its Python
    /// model.
    type Config: DeserializeOwned;

    /// The provider.
    type Provider: ImageProvider;

    /// The name the package asks for the vendor by, among image vendors.
    const NAME: &'static str;

    /// The name `RoomKit` records the provider under, which its
    /// [`name`](ImageProvider::name) must answer.
    const RECORDED_AS: &'static str;

    /// What the provider's failures carry as their provider.
    const LABEL: &'static str;

    /// `config` with `key` as its key: the key crosses apart from the JSON
    /// form, which writes it masked.
    fn keyed(config: Self::Config, key: Option<String>) -> Self::Config;

    /// The provider `config` configures.
    fn built(config: Self::Config) -> Result<Self::Provider, ProviderError>;

    /// `provider` as a replay of recorded cases drives it: sending to
    /// `fake`, a loopback address, its clock at `clock`.
    fn replaying(
        provider: Self::Provider,
        fake: SocketAddr,
        clock: f64,
    ) -> Result<(Self::Provider, ReplayWaits), ProviderError>;
}

/// The entries of one image vendor, as the module looks them up by name.
#[derive(Debug)]
pub(crate) struct ImageEntries {
    name: &'static str,
    label: &'static str,
    provider: fn(Python<'_>, &str, Option<String>) -> PyResult<PythonImageProvider>,
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
) -> PyResult<(PythonImageProvider, ReplayWaits)>;

impl ImageEntries {
    /// The entries of the image vendor `V`.
    pub(crate) const fn of<V: ImageVendor>() -> Self {
        Self {
            name: V::NAME,
            label: V::LABEL,
            provider: provider_of::<V>,
            catalogue: catalogue_of::<V>,
            replaying: replaying_of::<V>,
        }
    }
}

fn config_of<V: ImageVendor>(json: &str) -> PyResult<V::Config> {
    serde_json::from_str(json).map_err(|error| PyValueError::new_err(error.to_string()))
}

/// `provider` as Python holds it, once it names itself as its vendor
/// states.
fn held<V: ImageVendor>(py: Python<'_>, provider: V::Provider) -> PyResult<PythonImageProvider> {
    let held = PythonImageProvider::holding(provider, V::LABEL);
    let held = held.map_err(|failure| failure.raised(py))?;
    if held.name() == V::RECORDED_AS {
        Ok(held)
    } else {
        Err(Failure::misnamed(V::LABEL).raised(py))
    }
}

fn provider_of<V: ImageVendor>(
    py: Python<'_>,
    config_json: &str,
    key: Option<String>,
) -> PyResult<PythonImageProvider> {
    let config = V::keyed(config_of::<V>(config_json)?, key);
    let provider = V::built(config).map_err(|error| Failure::from(error).raised(py))?;
    held::<V>(py, provider)
}

fn catalogue_of<V: ImageVendor>(py: Python<'_>) -> PyResult<String> {
    let models = V::Provider::available_models();
    written(&models, "the catalogue").map_err(|failure| failure.raised(py))
}

fn replaying_of<V: ImageVendor>(
    py: Python<'_>,
    config_json: &str,
    key: Option<String>,
    fake: SocketAddr,
    clock: f64,
) -> PyResult<(PythonImageProvider, ReplayWaits)> {
    let config = V::keyed(config_of::<V>(config_json)?, key);
    let built = V::built(config).and_then(|provider| V::replaying(provider, fake, clock));
    let (provider, waits) = built.map_err(|error| Failure::from(error).raised(py))?;
    Ok((held::<V>(py, provider)?, waits))
}

/// The entries of the image vendor named `vendor`.
fn entries(vendor: &str) -> PyResult<&'static ImageEntries> {
    let held = vendors::IMAGE_VENDORS.iter();
    #[cfg(feature = "testing")]
    let held = held.chain(std::iter::once(&super::testing_image::IMAGE_VENDOR));
    let mut held = held;
    held.find(|entries| entries.name == vendor).ok_or_else(|| {
        PyValueError::new_err("the native module holds no image provider of that name")
    })
}

/// The image provider of `vendor` that the configuration `config_json`
/// writes configures, with the key `api_key` holds: the key crosses apart,
/// never inside the JSON text.
#[pyfunction]
fn image_provider(
    py: Python<'_>,
    vendor: &str,
    config_json: &str,
    api_key: &Bound<'_, PyAny>,
) -> PyResult<PythonImageProvider> {
    let entries = entries(vendor)?;
    guarded(py, entries.label, || {
        (entries.provider)(py, config_json, key_of(api_key)?)
    })
}

/// The curated catalogue of the image vendor `vendor` as a JSON list.
#[pyfunction]
fn image_catalogue(py: Python<'_>, vendor: &str) -> PyResult<String> {
    let entries = entries(vendor)?;
    guarded(py, entries.label, || (entries.catalogue)(py))
}

/// For the replay of the recorded cases, never for a host: the image
/// provider [`image_provider`] builds, sending to `host:port`, a loopback
/// address, over plain HTTP, its clock at `clock`.
#[pyfunction]
fn _image_replaying(
    py: Python<'_>,
    vendor: &str,
    config_json: &str,
    api_key: &Bound<'_, PyAny>,
    host: &str,
    port: u16,
    clock: f64,
) -> PyResult<(PythonImageProvider, PythonReplayWaits)> {
    let entries = entries(vendor)?;
    let host: IpAddr = host
        .parse()
        .map_err(|_| PyValueError::new_err("a replay's fake is named by its IP address"))?;
    guarded(py, entries.label, || {
        let fake = SocketAddr::new(host, port);
        let (provider, waits) =
            (entries.replaying)(py, config_json, key_of(api_key)?, fake, clock)?;
        Ok((provider, PythonReplayWaits::of(waits)))
    })
}

/// Adds the entries every image vendor gets to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(image_provider, module)?)?;
    module.add_function(wrap_pyfunction!(image_catalogue, module)?)?;
    module.add_function(wrap_pyfunction!(_image_replaying, module)?)?;
    module.add_class::<PythonImageProvider>()?;
    module.add_class::<super::image::PythonImageCall>()
}
