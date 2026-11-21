//! The providers the module holds, one module each: a provider's part is
//! its configuration, how it is built, and its catalogue. Everything a
//! built provider does crosses through the one class of `provider`.
//!
//! Adding a provider adds its module here, and its line in [`register`].

use pyo3::prelude::*;

mod openai;

/// Adds every provider's functions to the native module.
pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    openai::register(module)
}
