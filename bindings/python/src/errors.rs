//! How a failure crosses: as the exception a Python caller catches.
//!
//! A provider's failure is raised as the package's `ProviderFailure`, built
//! from its attributes, with the reason of a response schema's failure when
//! it is one. What the contract refuses as a value (a request that does not
//! read) is a `ValueError`. The class is Python's, defined in
//! `roomkit_providers.llm._failure`: the native module only raises it, and
//! the package makes it the exception its caller catches.

use std::error::Error as _;

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use roomkit_providers_llm::{ProviderError, ResponseSchemaError};

/// Why a call gave nothing, before it is a Python exception: a failure is
/// built off the interpreter's lock and raised under it.
#[derive(Debug)]
pub(crate) enum Failure {
    /// A value the contract refuses, in its reader's words.
    Value(String),
    /// A provider's failure.
    Provider(ProviderError),
}

impl From<ProviderError> for Failure {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl Failure {
    /// The exception Python raises for it.
    pub(crate) fn raised(self) -> PyErr {
        match self {
            Self::Value(said) => PyValueError::new_err(said),
            Self::Provider(error) => Python::attach(|py| {
                // A package whose error classes cannot be reached still
                // says what failed.
                provider_error(py, &error)
                    .unwrap_or_else(|_| PyRuntimeError::new_err(error.to_string()))
            }),
        }
    }
}

/// `error` as the package's exception: `ProviderFailure(message, **its
/// attributes)`, `schema_reason` being the reason of a response schema's
/// failure, None for any other.
fn provider_error(py: Python<'_>, error: &ProviderError) -> PyResult<PyErr> {
    let failures = py.import(intern!(py, "roomkit_providers.llm._failure"))?;
    let schema = error
        .source()
        .and_then(|source| source.downcast_ref::<ResponseSchemaError>());
    let attributes = PyDict::new(py);
    attributes.set_item(intern!(py, "retryable"), error.retryable)?;
    attributes.set_item(intern!(py, "provider"), &error.provider)?;
    attributes.set_item(intern!(py, "status_code"), error.status_code)?;
    attributes.set_item(intern!(py, "context_overflow"), error.context_overflow)?;
    attributes.set_item(intern!(py, "before_request"), error.before_request)?;
    let reason = schema.map(|schema| schema.reason.as_str());
    attributes.set_item(intern!(py, "schema_reason"), reason)?;
    let class = failures.getattr(intern!(py, "ProviderFailure"))?;
    let raised = class.call((error.message.as_str(),), Some(&attributes))?;
    Ok(PyErr::from_value(raised))
}
