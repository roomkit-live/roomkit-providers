//! What a failed request reads as on Ollama's API: how the body of a
//! refused request is read, and the failures of an answer in the
//! provider's own words (Python's `provider_error`, over the ollama SDK's
//! `ResponseError`).
//!
//! A failure is a [`Failure`] of `roomkit-providers-client`, which keeps
//! the provider's own words apart from what a server or a reader said. It
//! leaves the provider through that crate's `Exit`, where what was said is
//! cleaned of the configuration's secrets, put on one line and bounded.

pub(crate) use roomkit_providers_client::failure::{Failure, past_mark};
use roomkit_providers_client::failure::{MESSAGE_LIMIT, one_line, own, retryable_status, told};
use serde_json::Value;

/// A refused request: its status, retryable as the status is (429, 5xx),
/// and the `error` its body names, else the body. Python reads no overflow
/// of the model's context from Ollama's errors: none is stated.
pub(crate) fn status(status: u16, body: &[u8]) -> Failure {
    let mut said = vec![own(format!("the provider answered HTTP {status}"))];
    if let Some(error) = error_of(body) {
        said.extend([own(": "), told(error, MESSAGE_LIMIT)]);
    }
    Failure::said(said, retryable_status(status), Some(status))
}

/// What the body of a refused request says: the `error` of a JSON object,
/// or the body itself (the SDK's `ResponseError`).
fn error_of(body: &[u8]) -> Option<String> {
    let parsed: Option<Value> = serde_json::from_slice(past_mark(body)).ok();
    let stated = parsed.as_ref().and_then(|parsed| parsed.get("error"));
    let stated = stated.map(|error| {
        error
            .as_str()
            .map_or_else(|| error.to_string(), str::to_owned)
    });
    let text = stated.unwrap_or_else(|| String::from_utf8_lossy(body).into_owned());
    Some(text).filter(|text| !one_line(text).is_empty())
}

/// An error a stream wrote on a line of its own, the server aborting the
/// generation: retried as a lost connection, a retry generating anew (the
/// SDK's `ResponseError` with no status).
pub(crate) fn aborted(error: &Value) -> Failure {
    let error = error
        .as_str()
        .map_or_else(|| error.to_string(), str::to_owned);
    let said = vec![
        own("the provider's stream failed: "),
        told(error, MESSAGE_LIMIT),
    ];
    Failure::said(said, true, None)
}

/// A line of a stream that is no JSON object.
pub(crate) fn no_object() -> Failure {
    Failure::unreadable("a line of the stream is no JSON object")
}

/// An answer the SDK does not read as one.
pub(crate) fn unread(error: &serde_json::Error) -> Failure {
    Failure::malformed(error)
}

/// A listing that is no list of models.
pub(crate) fn no_listing() -> Failure {
    Failure::unreadable("the model listing is no list of models")
}
