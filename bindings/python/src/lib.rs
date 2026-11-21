//! Optional `CPython` bindings; what a provider sends and reads stays in
//! the Rust provider crates.
//!
//! The module is `roomkit_providers._native`, private to the Python
//! package, which wraps it: JSON text crosses both ways in the contract's
//! forms, a failure crosses as the package's exception, and every call
//! that waits returns an awaitable.

use pyo3::prelude::*;

mod errors;
mod fork;
mod openai;
mod runtime;

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    runtime::install(module)?;
    module.add_function(wrap_pyfunction!(openai::openai_config_form, module)?)?;
    module.add_class::<openai::PythonOpenAiProvider>()?;
    module.add_class::<openai::PythonStream>()?;
    Ok(())
}
