//! How a failure crosses: as the exception a Python caller catches.
//!
//! A provider's failure is raised as the package's `ProviderFailure`, built
//! from its attributes, with the reason of a response schema's failure when
//! it is one. A request the contract refuses, one that does not read, is
//! such a failure too, of before the request. The class is Python's, defined in
//! `roomkit_providers.llm._failure`: the native module only raises it, and
//! the package makes it the exception its caller catches.

use std::error::Error as _;
use std::panic::AssertUnwindSafe;

use pyo3::exceptions::PyRuntimeError;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use roomkit_providers_client::Secrets;
use roomkit_providers_llm::{ProviderError, ProviderErrorOptions, ResponseSchemaError};

/// The most characters of a reader's words a failure repeats: a reader
/// quotes what it refuses, however long.
const SAID_LIMIT: usize = 500;

/// Why a call gave nothing, before it is a Python exception. It holds no
/// Python reference: it is built on any thread, and made an exception
/// under the interpreter's lock.
#[derive(Debug)]
pub(crate) struct Failure(ProviderError);

impl From<ProviderError> for Failure {
    fn from(error: ProviderError) -> Self {
        Self(error)
    }
}

/// `said` on one line, bounded: each control character and each run of
/// white space one space, cut at [`SAID_LIMIT`] characters.
fn bounded(said: &str) -> String {
    let spaced: String = said
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    line.chars().take(SAID_LIMIT).collect()
}

impl Failure {
    /// A final failure of the provider labelled `label`, which no new
    /// request gets past.
    fn last(said: String, label: &str, before_request: bool) -> Self {
        let options = ProviderErrorOptions {
            provider: label.to_owned(),
            before_request,
            ..ProviderErrorOptions::default()
        };
        Self(ProviderError::new(said, options))
    }

    /// The failure of a request the contract refuses, in its reader's words
    /// `said`, bounded, for the provider labelled `label`: nothing was
    /// sent, and sending it again changes nothing.
    pub(crate) fn refused(said: &str, label: &str) -> Self {
        let said = bounded(&format!("the request is refused: {said}"));
        Self::last(said, label, true)
    }

    /// The failure of a call whose work panicked: what it did is unknown,
    /// and it is not asked again.
    pub(crate) fn panicked(label: &str) -> Self {
        let said = "the provider stopped on an internal error".to_owned();
        Self::last(said, label, false)
    }

    /// The failure of a provider that does not name itself as its vendor
    /// states: what `RoomKit` records of it would be wrong, so it is not
    /// built.
    pub(crate) fn misnamed(label: &str) -> Self {
        let said = "the provider cannot be built: it does not name itself as its vendor states";
        Self::last(said.to_owned(), label, true)
    }

    /// The failure of a stream whose reading stopped without saying how
    /// the stream ended: what came is no whole answer.
    pub(crate) fn unended(label: &str) -> Self {
        let said = "the provider's stream closed without its end".to_owned();
        Self::last(said, label, false)
    }

    /// The failure without `lent`, the key a host lent the one request
    /// that failed, in any form a failure's text may hold it in. A provider
    /// cleans its failures of that key itself; this holds for any provider,
    /// and for what refuses a request before a provider sees it.
    #[must_use]
    pub(crate) fn without(mut self, lent: Option<&str>) -> Self {
        if let Some(lent) = lent {
            let lent = Secrets::of([lent.to_owned()]);
            self.0.message = lent.clean(&self.0.message, usize::MAX);
        }
        self
    }

    /// The exception Python raises for it.
    pub(crate) fn raised(self, py: Python<'_>) -> PyErr {
        let Self(error) = self;
        // A package whose error class cannot be reached still says what
        // failed.
        provider_error(py, &error).unwrap_or_else(|_| PyRuntimeError::new_err(error.to_string()))
    }
}

/// `entry`, an entry into the native module that does not wait, its panic
/// made the failure of the provider labelled `label`. A defect of the
/// native code is a provider's failure wherever it shows: without this, a
/// panic leaves an entry as an exception that is no `Exception`, which a
/// caller's `except Exception` lets through.
pub(crate) fn guarded<T>(
    py: Python<'_>,
    label: &str,
    entry: impl FnOnce() -> PyResult<T>,
) -> PyResult<T> {
    match std::panic::catch_unwind(AssertUnwindSafe(entry)) {
        Ok(given) => given,
        Err(_) => Err(Failure::panicked(label).raised(py)),
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
