//! Optional `CPython` bindings; what a provider sends and reads stays in
//! the Rust provider crates.
//!
//! The module is `roomkit_providers._native`, private to the Python
//! package, which wraps it: JSON text crosses both ways in the contract's
//! forms, a failure crosses as the package's exception, and every call
//! that waits returns an awaitable.
//!
//! One class holds any provider of the contract (`provider`), with its
//! stream (`stream`): the crossing, cancellation, the interpreter's exit
//! and a forked process are written there once. So is every entry Python
//! calls for a vendor (`vendor`): a provider's own part is a module of
//! `vendors`, its configuration and how it is built.

use pyo3::prelude::*;

mod dynamic;
mod errors;
mod fork;
mod image;
mod image_vendor;
mod provider;
mod runtime;
mod stream;
#[cfg(feature = "testing")]
mod testing;
#[cfg(feature = "testing")]
mod testing_image;
mod vendor;
mod vendors;

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    runtime::install(module)?;
    module.add_class::<provider::PythonProvider>()?;
    module.add_class::<stream::PythonStream>()?;
    vendor::register(module)?;
    image_vendor::register(module)?;
    #[cfg(feature = "testing")]
    testing::register(module)?;
    Ok(())
}
